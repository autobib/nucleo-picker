use super::{
    Preview,
    cache::{Cached, RequestState},
    request::Request,
};
use lru::LruCache;
use nucleo_picker_core::{
    event::PreviewEvent,
    preview::{Buffer, Content},
    session::{Context, Dimensions, ItemId, ItemRef, Matches},
};
use std::{num::NonZero, time::Instant};

const PREFETCH_CHECKS_PER_FRAME: usize = 4;

pub(crate) struct PreviewState {
    cache: LruCache<ItemId, Cached>,
    prefetch: Option<Prefetch>,
    spare: Buffer,
    last_item: Option<ItemId>,
    // Unlike last_item, the hovered field is cleared while the pane is hidden.
    hovered: Option<ItemId>,
    // the epoch is advanced when the hovered item changes and is reset on restart. the epoch is passed to
    // each preview request and then returned by the previewer. it is used to prevent repeated
    // resubmission when the hovered item has not changed
    epoch: u64,
    // Events use the last applied geometry even when a layout change is pending.
    size: Dimensions,
}

impl PreviewState {
    pub(crate) fn new(cache_size: Option<NonZero<usize>>, prefetch_limit: usize) -> Self {
        Self {
            cache: cache_size.map_or_else(LruCache::unbounded, new_cache),
            prefetch: NonZero::new(prefetch_limit.saturating_mul(2)).map(|capacity| Prefetch {
                cache: new_cache(capacity),
                count: prefetch_limit,
                rank: None,
                dirty: false,
                next: 0,
                queued: None,
            }),
            spare: Buffer::new(),
            last_item: None,
            hovered: None,
            epoch: 0,
            size: Dimensions::default(),
        }
    }

    fn cached(&self) -> Option<&Cached> {
        self.last_item.and_then(|id| self.cache.peek(&id))
    }

    pub(crate) fn content(&self) -> Content<'_> {
        match self.cached().and_then(|cached| cached.state.as_ref()) {
            Some(RequestState::Ready(pane)) => Content::Ready(pane),
            Some(RequestState::Pending(_) | RequestState::Active(_) | RequestState::Retry(_)) => {
                Content::Loading
            }
            None => Content::Empty,
        }
    }

    fn set_hovered<T, P: Preview<T>>(&mut self, previewer: &mut P, item: Option<ItemRef<'_, T>>) {
        let hovered = item.map(|item| item.id());
        if self.hovered != hovered {
            self.hovered = hovered;
            if !std::thread::panicking() {
                previewer.hover_changed(item);
            }
        }
    }

    pub(crate) fn end<T, P: Preview<T>>(&mut self, previewer: &mut P) {
        self.cache.clear();
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.cache.clear();
        }
        self.set_hovered(previewer, None);
    }

    pub(crate) fn update<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        matches_changed: bool,
        deadline: Instant,
    ) -> bool {
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.dirty |= matches_changed;
        }
        if !cx.active_components().preview {
            self.set_hovered(previewer, None);
            return false;
        }
        let redraw = self.update_current(previewer, cx, deadline);
        self.sync_prefetch(cx.hovered_rank());
        redraw
    }

    /// Update the current preview by polling and resubmitting requests and keeping track if a
    /// redraw is required.
    fn update_current<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) -> bool {
        let item = cx.hovered();
        let hovered = item.map(|item| item.id());
        if self.hovered != hovered {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.set_hovered(previewer, item);
        let changed = self.last_item != hovered;
        self.last_item = hovered;
        let Some(item) = item else {
            return changed;
        };
        let id = item.id();

        if !self.cache.contains(&id) {
            let cached = self
                .prefetch
                .as_mut()
                .and_then(|prefetch| prefetch.cache.pop(&id))
                .unwrap_or_default();
            if let Some((_, evicted)) = self.cache.push(id, cached) {
                self.spare = evicted.into_buffer();
            }
        }
        let cached = self.cache.get_mut(&id).unwrap();
        if matches!(cached.state, Some(RequestState::Ready(_))) {
            return changed;
        }

        // check the state:
        //
        // - if ready, we're done
        // - if pending, check if the epoch is different. if so, reprioritize it by
        //   cancelling and resubmitting
        // - if the epoch is the same, or a worker is currently handing the request,
        //   wait
        // - if dropped, recover the buffer and resubmit
        let state = match cached.state.take() {
            Some(state) => state.poll().reprioritize(self.epoch),
            None => RequestState::Retry(std::mem::take(&mut self.spare)),
        };
        let state = match state {
            RequestState::Retry(buffer) => {
                submit(previewer, cx.matches(), id, buffer, self.epoch, deadline).poll()
            }
            state => state,
        };
        let ready = matches!(state, RequestState::Ready(_));
        cached.state = Some(state);
        changed || ready
    }

    fn sync_prefetch(&mut self, rank: Option<u32>) {
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        // a hovered prefetch was already moved to the main cache by update_current
        if prefetch.rank != rank || prefetch.dirty {
            prefetch.rank = rank;
            prefetch.next = 0;
            if let Some(id) = prefetch.queued.take()
                && let Some(cached) = prefetch.cache.peek_mut(&id)
            {
                cached.state = cached.state.take().map(RequestState::cancel_queued);
            }
        }
        prefetch.dirty = false;
    }

    pub(crate) fn update_prefetch<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) {
        if !cx.active_components().preview {
            return;
        }
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        let Some(rank) = prefetch.rank else {
            return;
        };
        let matches = cx.matches();
        let total = matches.len() as u32;
        let checks = PREFETCH_CHECKS_PER_FRAME
            .min(prefetch.window_len(total).saturating_sub(prefetch.next) as usize);
        if checks == 0 {
            return;
        }
        if !self
            .hovered
            .and_then(|id| self.cache.peek(&id))
            .is_some_and(|cached| {
                matches!(
                    cached.state,
                    Some(RequestState::Ready(_) | RequestState::Active(_))
                )
            })
        {
            return;
        }

        if let Some(id) = prefetch.queued.take()
            && let Some(cached) = prefetch.cache.peek_mut(&id)
        {
            cached.state = cached.state.take().map(RequestState::poll);
            if matches!(cached.state, Some(RequestState::Pending(_))) {
                prefetch.queued = Some(id);
                return;
            }
        }

        for _ in 0..checks {
            let budget = deadline.saturating_duration_since(Instant::now());
            if budget.is_zero() {
                return;
            }
            let candidate = prefetch.next(rank, total).unwrap();
            let id = matches.ids().nth(candidate as usize).unwrap();
            if self.cache.contains(&id) {
                continue;
            }

            let mut buffer = if let Some(cached) = prefetch.cache.get_mut(&id) {
                match cached.state.take().map(RequestState::poll) {
                    Some(RequestState::Retry(buffer)) => buffer,
                    None => std::mem::take(&mut self.spare),
                    state => {
                        cached.state = state;
                        continue;
                    }
                }
            } else {
                std::mem::take(&mut self.spare)
            };
            buffer.clear();
            let request = Request {
                buffer,
                epoch: self.epoch,
                matches,
                id,
            };
            let state = match previewer.prefetch(request, budget) {
                Ok(response) => {
                    let state = RequestState::from(response).poll();
                    if matches!(state, RequestState::Pending(_)) {
                        prefetch.queued = Some(id);
                    }
                    state
                }
                Err(buffer) => {
                    if let Some(cached) = prefetch.cache.peek_mut(&id) {
                        cached.state = Some(RequestState::Retry(buffer));
                    } else {
                        self.spare = buffer;
                    }
                    continue;
                }
            };
            if let Some((evicted_id, evicted)) =
                prefetch.cache.push(id, Cached { state: Some(state) })
                && evicted_id != id
            {
                self.spare = evicted.into_buffer();
            }
            return;
        }
    }

    pub(crate) fn handle_event(&mut self, id: Option<ItemId>, event: PreviewEvent) -> bool {
        let Some(cached) = id.and_then(|id| {
            self.cache.get_mut(&id).or_else(|| {
                if matches!(event, PreviewEvent::Refresh) {
                    self.prefetch.as_mut()?.cache.peek_mut(&id)
                } else {
                    None
                }
            })
        }) else {
            return false;
        };

        // handle refresh first since we will refresh / cancel
        // even if the preview is not ready yet
        if matches!(event, PreviewEvent::Refresh) {
            cached.state = cached
                .state
                .take()
                .map(|state| RequestState::Retry(state.into_buffer()));
            return self.last_item == id;
        }

        // then check: is the preview ready? if not, nothing to do
        let Some(pane) = cached.pane_mut() else {
            return false;
        };

        let previous = (pane.offset(), pane.line_numbers());
        match event {
            PreviewEvent::ToggleLineNumbers => pane.toggle_line_numbers(),
            PreviewEvent::SetLineNumbers(enabled) => pane.set_line_numbers(enabled),
            _ if self.size.width == 0 => return false,
            event => {
                let mut viewport = pane.viewport(self.size);
                match event {
                    PreviewEvent::Left(columns) => viewport.left(columns),
                    PreviewEvent::Right(columns) => viewport.right(columns),
                    PreviewEvent::Up(lines) => viewport.up(lines),
                    PreviewEvent::Down(lines) => viewport.down(lines),
                    PreviewEvent::PageUp(pages) => viewport.page_up(pages),
                    PreviewEvent::PageDown(pages) => viewport.page_down(pages),
                    PreviewEvent::AlignLeft => viewport.align_left(),
                    PreviewEvent::AlignRight => viewport.align_right(),
                    PreviewEvent::AlignTop => viewport.align_top(),
                    PreviewEvent::AlignBottom => viewport.align_bottom(),
                    PreviewEvent::Refresh
                    | PreviewEvent::ToggleLineNumbers
                    | PreviewEvent::SetLineNumbers(_) => unreachable!(),
                    _ => return false,
                };
            }
        }
        (pane.offset(), pane.line_numbers()) != previous
    }

    pub(crate) fn resize(&mut self, size: Option<Dimensions>) {
        let size = size.unwrap_or_default();
        self.size = size;
        for (_, cached) in self.cache.iter_mut() {
            if let Some(pane) = cached.pane_mut() {
                pane.viewport(size).clamp_vertical();
            }
        }
    }

    pub(crate) fn restart<T, P: Preview<T>>(&mut self, previewer: &mut P) {
        self.last_item = None;
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.rank = None;
            prefetch.dirty = false;
            prefetch.queued = None;
        }
        self.spare = Buffer::new();
        self.epoch = 0;
        self.end(previewer);
        previewer.restart();
    }
}

fn new_cache(capacity: NonZero<usize>) -> LruCache<ItemId, Cached> {
    if capacity.get() <= 256 {
        LruCache::new(capacity)
    } else {
        LruCache::sparse(capacity)
    }
}

fn submit<T: Send + Sync + 'static, P: Preview<T>>(
    previewer: &mut P,
    matches: Matches<'_, T>,
    id: ItemId,
    mut buffer: Buffer,
    epoch: u64,
    deadline: Instant,
) -> RequestState {
    buffer.clear();
    let request = Request {
        buffer,
        epoch,
        matches,
        id,
    };
    previewer
        .preview(request, deadline.saturating_duration_since(Instant::now()))
        .into()
}

struct Prefetch {
    cache: LruCache<ItemId, Cached>,
    count: usize,
    rank: Option<u32>,
    dirty: bool,
    next: u32,
    queued: Option<ItemId>,
}

impl Prefetch {
    fn window_len(&self, total: u32) -> u32 {
        self.count.min(total.saturating_sub(1) as usize) as u32
    }

    fn next(&mut self, rank: u32, total: u32) -> Option<u32> {
        if self.next >= self.window_len(total) {
            return None;
        }
        let next = self.next;
        self.next += 1;

        let before = rank;
        let after = total - rank - 1;
        let paired = before.min(after);
        // interleave until hitting then boundary and then only use the
        // remaining direction
        Some(if next < 2 * paired {
            let distance = next / 2 + 1;
            if next.is_multiple_of(2) {
                rank + distance
            } else {
                rank - distance
            }
        } else {
            let distance = next - paired + 1;
            if after > before {
                rank + distance
            } else {
                rank - distance
            }
        })
    }
}

#[cfg(test)]
mod tests;
