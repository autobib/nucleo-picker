use super::{
    BoundaryChars, ItemId, ItemRef, Preview, PreviewBuffer, PreviewConfig, PreviewEvent,
    cache::{Cached, RequestState},
    request::{PreviewRequest, PreviewResponse},
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
    time::{Duration, Instant},
};

pub(crate) struct PreviewPane<T, P: Preview<T>> {
    // fields drop in declaration order: cancel requests *before* dropping the
    // previewer so that it may join its workers if desired
    cache: LruCache<u32, Cached>,
    previewer: P,
    last_item: Option<u32>,
    // Unlike last_item, focused is cleared while the pane is hidden.
    focused: Option<u32>,
    item: PhantomData<fn() -> T>,
    // state to preserve scroll and restart invalidation and return redraw state with
    // `update`
    pending_redraw: bool,
    // the epoch is advanced when the item changes and is reset on restart. the epoch is passed to
    // each preview request and then returned by the previewer. it is used to prevent repeated
    // resubmission when the cursor has not moved
    epoch: u64,
    area: Area,
    boundary_chars: BoundaryChars,
    ellipsis: char,
}

impl<T, P: Preview<T>> PreviewPane<T, P> {
    pub(crate) fn new(config: &PreviewConfig, chars: &PickerChars, previewer: P) -> Self {
        Self {
            cache: config
                .cache_size
                .map_or_else(LruCache::unbounded, LruCache::new),
            previewer,
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
        self.set_focus(selected.map(|idx| engine.item_ref(idx).unwrap()));
        if self.last_item != selected {
            self.last_item = selected;
            self.epoch = self.epoch.wrapping_add(1);
            self.pending_redraw = true;
        }
        let Some(idx) = selected else {
            return;
        };

        let Some(cached) = self.cache.get_mut(&idx) else {
            let evicted = self.cache.push(
                idx,
                Cached {
                    scroll_position: 0,
                    horizontal_position: 0,
                    line_numbers_override: None,
                    state: None,
                },
            );
            let buffer = evicted
                .and_then(|(_, mut cached)| cached.state.take())
                .map_or_else(PreviewBuffer::new, RequestState::into_buffer);
            let state = submit(
                &mut self.previewer,
                snapshot,
                engine.item_id(idx),
                buffer,
                self.epoch,
                deadline,
            );
            self.pending_redraw |= matches!(state, RequestState::Ready(_));
            self.cache.peek_mut(&idx).unwrap().state = Some(state);
            return;
        };
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
            Some(state) => state.poll(self.epoch),
            None => RequestState::Retry(PreviewBuffer::new()),
        };
        let state = match state {
            RequestState::Retry(buffer) => submit(
                &mut self.previewer,
                snapshot,
                engine.item_id(idx),
                buffer,
                self.epoch,
                deadline,
            ),
            state => state,
        };
        self.pending_redraw |= matches!(state, RequestState::Ready(_));
        cached.state = Some(state);
    }
}

impl<T, P: Preview<T>> PreviewPane<T, P> {
    fn handle_event(&mut self, idx: Option<u32>, event: PreviewEvent) {
        let Some(cached) = idx.and_then(|idx| self.cache.get_mut(&idx)) else {
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
        selected: Option<u32>,
        engine: &MatchEngine<T, R>,
        deadline: Instant,
    ) -> bool {
        self.update_current(selected, engine, deadline);
        std::mem::take(&mut self.pending_redraw)
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
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(2));
    match previewer.preview(request, timeout) {
        PreviewResponse::Ready(buffer) => RequestState::Ready(buffer),
        PreviewResponse::Pending(pending) => RequestState::Pending(pending),
    }
}

#[cfg(test)]
pub(crate) mod tests;
