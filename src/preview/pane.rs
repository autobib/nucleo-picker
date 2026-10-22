use super::{
    BoundaryChars, ItemId, ItemRef, Preview, PreviewBuffer, PreviewConfig, PreviewEvent,
    cache::{Cached, RequestState},
    request::PreviewRequest,
    scroll::PreviewMovement,
};
use crate::{
    PickerChars,
    component::{Component, PreviewComponent},
    match_engine::MatchEngine,
    rect::{Area, Rect},
};
use lru::LruCache;
use std::{
    io,
    marker::PhantomData,
    num::NonZero,
    time::{Duration, Instant},
};

const PREFETCH_CHECKS_PER_FRAME: usize = 4;

pub(crate) struct PreviewPane<T, P: Preview<T>> {
    // fields drop in declaration order: cancel requests *before* dropping the
    // previewer so that it may join its workers if desired
    cache: LruCache<u32, Cached>,
    prefetch: Option<Prefetch>,
    previewer: P,
    spare: PreviewBuffer,
    last_item: Option<u32>,
    // Unlike last_item, focused is cleared while the pane is hidden.
    focused: Option<u32>,
    item: PhantomData<fn() -> T>,
    // state to preserve scroll and restart invalidation and return redraw state with
    // `update`
    pending_redraw: bool,
    // the epoch is advanced when the focus changes and is reset on restart. the epoch is passed to
    // each preview request and then returned by the previewer. it is used to prevent repeated
    // resubmission when the cursor has not moved
    epoch: u64,
    area: Area,
    boundary_chars: BoundaryChars,
    ellipsis: char,
}

impl<T, P: Preview<T>> PreviewPane<T, P> {
    pub(crate) fn new(config: &PreviewConfig, chars: &PickerChars, previewer: P) -> Self {
        let prefetch_limit = previewer.prefetch_limit();
        Self {
            cache: config
                .cache_size
                .map_or_else(LruCache::unbounded, new_cache),
            prefetch: NonZero::new(prefetch_limit.saturating_mul(2)).map(|capacity| Prefetch {
                cache: new_cache(capacity),
                count: prefetch_limit,
                cycle: None,
                next: 0,
                queued: None,
            }),
            previewer,
            spare: PreviewBuffer::new(),
            last_item: None,
            focused: None,
            item: PhantomData,
            pending_redraw: false,
            epoch: 0,
            area: Area::default(),
            boundary_chars: config.boundary_chars,
            ellipsis: chars.ellipsis,
        }
    }

    #[cfg(test)]
    fn cached(&self) -> Option<&Cached> {
        self.last_item.and_then(|idx| self.cache.peek(&idx))
    }

    fn set_focus(&mut self, item: Option<ItemRef<'_, T>>) {
        let focused = item.map(|item| item.id().index);
        if self.focused != focused {
            self.focused = focused;
            if !std::thread::panicking() {
                self.previewer.focus_changed(item);
            }
        }
    }
}

impl<T, P: Preview<T>> Drop for PreviewPane<T, P> {
    fn drop(&mut self) {
        self.cache.clear();
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.cache.clear();
        }
        self.set_focus(None);
    }
}

impl<T: Send + Sync + 'static, P: Preview<T>> PreviewPane<T, P> {
    /// Update the current preview by polling and resubmitting requests and keeping track if a
    /// redraw is required.
    fn update_current<R>(
        &mut self,
        selected: Option<u32>,
        engine: &MatchEngine<T, R>,
        deadline: Instant,
    ) {
        let snapshot = engine.snapshot();
        if self.focused != selected {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.set_focus(selected.map(|idx| engine.item_ref(idx).unwrap()));
        if self.last_item != selected {
            self.last_item = selected;
            self.pending_redraw = true;
        }
        let Some(idx) = selected else {
            return;
        };

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
                &mut self.previewer,
                snapshot,
                engine.item_id(idx),
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

    fn sync_prefetch(&mut self, selected_rank: Option<u32>, revision: u64) {
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        // a selected prefetch was already moved to the main cache by update_current
        let cycle = selected_rank.map(|rank| (rank, revision));
        if prefetch.cycle != cycle {
            prefetch.cycle = cycle;
            prefetch.next = 0;
            if let Some(idx) = prefetch.queued.take()
                && let Some(cached) = prefetch.cache.peek_mut(&idx)
            {
                cached.state = cached.state.take().map(RequestState::cancel_queued);
            }
        }
    }

    fn update_prefetch<R>(&mut self, engine: &MatchEngine<T, R>, deadline: Instant) {
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        let Some((rank, _)) = prefetch.cycle else {
            return;
        };
        let snapshot = engine.snapshot();
        let total = snapshot.matched_item_count();
        let checks = PREFETCH_CHECKS_PER_FRAME
            .min(prefetch.window_len(total).saturating_sub(prefetch.next) as usize);
        if checks == 0 {
            return;
        }
        if !self
            .focused
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
            let idx = engine.idx_from_match(candidate);
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
            let request = PreviewRequest {
                buffer,
                epoch: self.epoch,
                snapshot,
                id: engine.item_id(idx),
            };
            let state = match self.previewer.prefetch(request, budget) {
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
            if let Some((evicted_idx, evicted)) = prefetch.cache.push(
                idx,
                Cached {
                    state: Some(state),
                    ..Cached::default()
                },
            ) && evicted_idx != idx
            {
                self.spare = evicted.into_buffer();
            }
            return;
        }
    }
}

impl<T, P: Preview<T>> PreviewPane<T, P> {
    fn handle_event(&mut self, idx: Option<u32>, event: PreviewEvent) {
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
            cached.scroll_position = 0;
            cached.horizontal_position = 0;
            cached.line_numbers_override = None;
            cached.state = cached
                .state
                .take()
                .map(|state| RequestState::Retry(state.into_buffer()));
            return;
        }

        // then check: is the preview ready? if not, nothing to do
        if !matches!(cached.state, Some(RequestState::Ready(_))) {
            return;
        }

        // handle other movement events
        let movement = match event {
            PreviewEvent::ToggleLineNumbers => {
                cached.line_numbers_override = Some(!cached.line_numbers());
                self.pending_redraw = true;
                return;
            }
            PreviewEvent::SetLineNumbers(enabled) => {
                let previous = cached.line_numbers();
                cached.line_numbers_override = enabled;
                self.pending_redraw |= previous != cached.line_numbers();
                return;
            }
            PreviewEvent::Left(columns) => PreviewMovement::Left(columns),
            PreviewEvent::Right(columns) => PreviewMovement::Right(columns),
            PreviewEvent::AlignLeft => PreviewMovement::AlignLeft,
            PreviewEvent::AlignRight => PreviewMovement::AlignRight,
            PreviewEvent::Up(lines) => PreviewMovement::Up(lines),
            PreviewEvent::Down(lines) => PreviewMovement::Down(lines),
            PreviewEvent::PageUp(pages) => PreviewMovement::PageUp(pages),
            PreviewEvent::PageDown(pages) => PreviewMovement::PageDown(pages),
            PreviewEvent::AlignTop => PreviewMovement::AlignTop,
            PreviewEvent::AlignBottom => PreviewMovement::AlignBottom,
            PreviewEvent::Refresh => return,
        };
        if !self.area.is_empty() {
            self.pending_redraw |= cached.scroll(
                movement,
                (
                    self.area.width.saturating_sub(2),
                    self.area.height.saturating_sub(2),
                ),
            );
        }
    }

    fn resize_area(&mut self, area: Area) {
        self.area = area;
        let height = if area.is_empty() {
            0
        } else {
            area.height.saturating_sub(2)
        };
        for (_, cached) in self.cache.iter_mut() {
            cached.resize(height);
        }
    }

    fn restart_cache(&mut self) {
        self.pending_redraw |= self.last_item.take().is_some();
        self.cache.clear();
        if let Some(prefetch) = &mut self.prefetch {
            prefetch.cache.clear();
            prefetch.cycle = None;
            prefetch.queued = None;
        }
        self.spare = PreviewBuffer::new();
        self.epoch = 0;
    }
}

impl<E, T, P: Preview<T>> Component<E> for PreviewPane<T, P> {
    fn resize(&mut self, area: Area, _engine: &E) {
        self.resize_area(area);
    }
    fn draw<D: Rect>(&mut self, _engine: &E, rect: &mut D) -> io::Result<()> {
        let preview = self.last_item.and_then(|idx| self.cache.peek(&idx));
        super::draw::draw(rect, preview, self.boundary_chars, self.ellipsis)
    }
}

impl<T: Send + Sync + 'static, P: Preview<T>> PreviewComponent<T> for PreviewPane<T, P> {
    const ENABLED: bool = true;

    fn handle(&mut self, event: PreviewEvent, selected_id: Option<u32>) {
        self.handle_event(selected_id, event);
    }

    fn update<R>(
        &mut self,
        selected_rank: Option<u32>,
        engine: &MatchEngine<T, R>,
        deadline: Instant,
    ) -> bool {
        let selected = selected_rank.map(|rank| engine.idx_from_match(rank));
        self.update_current(selected, engine, deadline);
        self.sync_prefetch(selected_rank, engine.match_revision());
        std::mem::take(&mut self.pending_redraw)
    }

    fn prefetch<R>(&mut self, engine: &MatchEngine<T, R>, deadline: Instant) {
        self.update_prefetch(engine, deadline);
    }

    fn hide(&mut self) {
        self.set_focus(None);
    }

    fn restart(&mut self) {
        self.restart_cache();
        self.set_focus(None);
        self.previewer.restart();
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
    mut buffer: PreviewBuffer,
    epoch: u64,
    deadline: Instant,
) -> RequestState {
    buffer.clear();
    let request = PreviewRequest {
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
    cycle: Option<(u32, u64)>,
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
        // Interleave both sides until the nearer boundary, then use only the remaining side.
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
