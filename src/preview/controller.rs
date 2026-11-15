use super::{
    BoundaryChars, Buffer, Content, ItemId, ItemRef, Preview, PreviewConfig, PreviewEvent,
    cache::{Cached, RequestState},
    interior_dimensions,
    request::Request,
};
use crate::{
    PickerChars,
    component::{Component, PreviewComponent},
    rect::{Area, Rect},
    session::{Context, Dimensions},
};
use lru::LruCache;
use std::{
    io,
    marker::PhantomData,
    num::NonZero,
    time::{Duration, Instant},
};

const PREFETCH_CHECKS_PER_FRAME: usize = 4;

pub(crate) struct PreviewController<T, P: Preview<T>> {
    state: PreviewState,
    previewer: P,
    item: PhantomData<fn() -> T>,
    boundary_chars: BoundaryChars,
    ellipsis: char,
}

struct PreviewState {
    // fields drop in declaration order: cancel requests *before* dropping the
    // previewer so that it may join its workers if desired
    cache: LruCache<u32, Cached>,
    prefetch: Option<Prefetch>,
    spare: Buffer,
    last_item: Option<u32>,
    // Unlike last_item, the hovered field is cleared while the pane is hidden.
    hovered: Option<u32>,
    // state to preserve scroll and restart invalidation and return redraw state with
    // `update`
    pending_redraw: bool,
    // the epoch is advanced when the hovered item changes and is reset on restart. the epoch is passed to
    // each preview request and then returned by the previewer. it is used to prevent repeated
    // resubmission when the hovered item has not changed
    epoch: u64,
    area: Area,
}

impl<T, P: Preview<T>> PreviewController<T, P> {
    pub(crate) fn new(
        config: &PreviewConfig,
        chars: &PickerChars,
        cache_size: Option<NonZero<usize>>,
        previewer: P,
    ) -> Self {
        Self {
            state: PreviewState::new(cache_size, previewer.prefetch_limit()),
            previewer,
            item: PhantomData,
            boundary_chars: config.boundary_chars,
            ellipsis: chars.ellipsis,
        }
    }
}

impl<T, P: Preview<T>> Drop for PreviewController<T, P> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.state.end(&mut self.previewer);
        }
    }
}

impl PreviewState {
    fn new(cache_size: Option<NonZero<usize>>, prefetch_limit: usize) -> Self {
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
            pending_redraw: false,
            epoch: 0,
            area: Area::default(),
        }
    }

    fn cached(&self) -> Option<&Cached> {
        self.last_item.and_then(|idx| self.cache.peek(&idx))
    }

    fn content(&self) -> Content<'_> {
        match self.cached().and_then(|cached| cached.state.as_ref()) {
            Some(RequestState::Ready(pane)) => Content::Ready(pane),
            Some(RequestState::Pending(_) | RequestState::Active(_) | RequestState::Retry(_)) => {
                Content::Loading
            }
            None => Content::Empty,
        }
    }

    fn set_hovered<T, P: Preview<T>>(&mut self, previewer: &mut P, item: Option<ItemRef<'_, T>>) {
        let hovered = item.map(|item| item.id().index);
        if self.hovered != hovered {
            self.hovered = hovered;
            if !std::thread::panicking() {
                previewer.hover_changed(item);
            }
        }
    }

    fn end<T, P: Preview<T>>(&mut self, previewer: &mut P) {
        self.cache.clear();
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.cache.clear();
        }
        self.set_hovered(previewer, None);
    }

    fn update<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        matches_changed: bool,
        deadline: Instant,
    ) -> bool {
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.dirty |= matches_changed;
        }
        if cx.preview_dimensions().is_none() {
            return false;
        }
        self.update_current(previewer, cx, deadline);
        self.sync_prefetch(cx.rank());
        std::mem::take(&mut self.pending_redraw)
    }

    /// Update the current preview by polling and resubmitting requests and keeping track if a
    /// redraw is required.
    fn update_current<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) {
        let item = cx.hovered();
        let hovered = item.map(|item| item.id().index);
        if self.hovered != hovered {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.set_hovered(previewer, item);
        if self.last_item != hovered {
            self.last_item = hovered;
            self.pending_redraw = true;
        }
        let Some(item) = item else {
            return;
        };
        let id = item.id();
        let idx = id.index;

        if !self.cache.contains(&idx) {
            let cached = self
                .prefetch
                .as_mut()
                .and_then(|prefetch| prefetch.cache.pop(&idx))
                .unwrap_or_default();
            if let Some((_, evicted)) = self.cache.push(idx, cached) {
                self.spare = evicted.into_buffer();
            }
        }
        let cached = self.cache.get_mut(&idx).unwrap();
        if matches!(cached.state, Some(RequestState::Ready(_))) {
            return;
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
            RequestState::Retry(buffer) => submit(
                previewer,
                cx.matches().snapshot,
                id,
                buffer,
                self.epoch,
                deadline,
            )
            .poll(),
            state => state,
        };
        self.pending_redraw |= matches!(state, RequestState::Ready(_));
        cached.state = Some(state);
    }

    fn sync_prefetch(&mut self, rank: Option<u32>) {
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        // a hovered prefetch was already moved to the main cache by update_current
        if prefetch.rank != rank || prefetch.dirty {
            prefetch.rank = rank;
            prefetch.next = 0;
            if let Some(idx) = prefetch.queued.take()
                && let Some(cached) = prefetch.cache.peek_mut(&idx)
            {
                cached.state = cached.state.take().map(RequestState::cancel_queued);
            }
        }
        prefetch.dirty = false;
    }

    fn update_prefetch<T: Send + Sync + 'static, P: Preview<T>>(
        &mut self,
        previewer: &mut P,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) {
        if cx.preview_dimensions().is_none() {
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
            .and_then(|idx| self.cache.peek(&idx))
            .is_some_and(|cached| {
                matches!(
                    cached.state,
                    Some(RequestState::Ready(_) | RequestState::Active(_))
                )
            })
        {
            return;
        }

        if let Some(idx) = prefetch.queued.take()
            && let Some(cached) = prefetch.cache.peek_mut(&idx)
        {
            cached.state = cached.state.take().map(RequestState::poll);
            if matches!(cached.state, Some(RequestState::Pending(_))) {
                prefetch.queued = Some(idx);
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
            let idx = id.index;
            if self.cache.contains(&idx) {
                continue;
            }

            let mut buffer = if let Some(cached) = prefetch.cache.get_mut(&idx) {
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
                snapshot: matches.snapshot,
                id,
            };
            let state = match previewer.prefetch(request, budget) {
                Ok(response) => {
                    let state = RequestState::from(response).poll();
                    if matches!(state, RequestState::Pending(_)) {
                        prefetch.queued = Some(idx);
                    }
                    state
                }
                Err(buffer) => {
                    if let Some(cached) = prefetch.cache.peek_mut(&idx) {
                        cached.state = Some(RequestState::Retry(buffer));
                    } else {
                        self.spare = buffer;
                    }
                    continue;
                }
            };
            if let Some((evicted_idx, evicted)) =
                prefetch.cache.push(idx, Cached { state: Some(state) })
                && evicted_idx != idx
            {
                self.spare = evicted.into_buffer();
            }
            return;
        }
    }

    fn handle_event(&mut self, idx: Option<u32>, event: PreviewEvent, size: Dimensions) {
        let Some(cached) = idx.and_then(|idx| {
            self.cache.get_mut(&idx).or_else(|| {
                if matches!(event, PreviewEvent::Refresh) {
                    self.prefetch.as_mut()?.cache.peek_mut(&idx)
                } else {
                    None
                }
            })
        }) else {
            return;
        };

        // handle refresh first since we will refresh / cancel
        // even if the preview is not ready yet
        if matches!(event, PreviewEvent::Refresh) {
            self.pending_redraw |= self.last_item == idx;
            cached.state = cached
                .state
                .take()
                .map(|state| RequestState::Retry(state.into_buffer()));
            return;
        }

        // then check: is the preview ready? if not, nothing to do
        let Some(pane) = cached.pane_mut() else {
            return;
        };

        let previous = (pane.offset(), pane.line_numbers());
        match event {
            PreviewEvent::ToggleLineNumbers => pane.toggle_line_numbers(),
            PreviewEvent::SetLineNumbers(enabled) => pane.set_line_numbers(enabled),
            _ if size.width == 0 => return,
            event => {
                let mut viewport = pane.viewport(size);
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
                };
            }
        }
        self.pending_redraw |= (pane.offset(), pane.line_numbers()) != previous;
    }

    fn resize_area(&mut self, area: Area) {
        self.area = area;
        let size = interior_dimensions(area);
        for (_, cached) in self.cache.iter_mut() {
            if let Some(pane) = cached.pane_mut() {
                pane.viewport(size).clamp_vertical();
            }
        }
    }

    fn restart<T, P: Preview<T>>(&mut self, previewer: &mut P) {
        self.pending_redraw |= self.last_item.take().is_some();
        self.cache.clear();
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.cache.clear();
            prefetch.rank = None;
            prefetch.dirty = false;
            prefetch.queued = None;
        }
        self.spare = Buffer::new();
        self.epoch = 0;
        self.set_hovered(previewer, None);
        previewer.restart();
    }
}

impl<T, P: Preview<T>> Component<()> for PreviewController<T, P> {
    fn resize(&mut self, area: Area, _engine: &()) {
        self.state.resize_area(area);
    }
    fn draw<D: Rect>(&mut self, _engine: &(), rect: &mut D) -> io::Result<()> {
        super::draw::draw(
            rect,
            self.state.content(),
            self.boundary_chars,
            self.ellipsis,
        )
    }
}

impl<T: Send + Sync + 'static, P: Preview<T>> PreviewComponent<T> for PreviewController<T, P> {
    const ENABLED: bool = true;

    fn handle(&mut self, event: PreviewEvent, cx: &Context<'_, T>) {
        let Some(size) = cx.preview_dimensions() else {
            return;
        };
        self.state
            .handle_event(cx.hovered().map(|item| item.id().index), event, size);
    }

    fn update(&mut self, cx: &Context<'_, T>, matches_changed: bool, deadline: Instant) -> bool {
        self.state
            .update(&mut self.previewer, cx, matches_changed, deadline)
    }

    fn prefetch(&mut self, cx: &Context<'_, T>, deadline: Instant) {
        self.state
            .update_prefetch(&mut self.previewer, cx, deadline);
    }

    fn hide(&mut self) {
        self.state.set_hovered(&mut self.previewer, None);
    }

    fn restart(&mut self) {
        self.state.restart(&mut self.previewer);
    }
}

fn new_cache(capacity: NonZero<usize>) -> LruCache<u32, Cached> {
    if capacity.get() <= 256 {
        LruCache::new(capacity)
    } else {
        LruCache::sparse(capacity)
    }
}

fn submit<T: Send + Sync + 'static, P: Preview<T>>(
    previewer: &mut P,
    snapshot: &nucleo::Snapshot<T>,
    id: ItemId,
    mut buffer: Buffer,
    epoch: u64,
    deadline: Instant,
) -> RequestState {
    buffer.clear();
    let request = Request {
        buffer,
        epoch,
        snapshot,
        id,
    };
    previewer.preview(request, timeout(deadline)).into()
}

fn timeout(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(2))
}

struct Prefetch {
    cache: LruCache<u32, Cached>,
    count: usize,
    rank: Option<u32>,
    dirty: bool,
    next: u32,
    queued: Option<u32>,
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
pub(crate) mod tests;
