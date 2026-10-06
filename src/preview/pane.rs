use super::{
    BoundaryChars, Preview, PreviewBuffer, PreviewConfig, PreviewEvent,
    cache::{BufferNotReady, Cached, RequestState},
    request::{PreviewRequest, PreviewResponse},
};
use crate::{
    PickerChars,
    component::{Component, PreviewComponent},
    rect::{Area, Rect},
};
use lru::LruCache;
use std::{
    io,
    time::{Duration, Instant},
};

pub(crate) struct PreviewPane<P> {
    // fields drop in declaration order: cancel requests *before* dropping the
    // previewer so that it may join its workers if desired
    cache: LruCache<u32, Cached>,
    previewer: P,
    last_item: Option<u32>,
    // state to preserve scroll and restart invalidation and return redraw sate with
    // `update`
    pending_redraw: bool,
    // the epoch is advanced when the item changes and is reset on restart. the epoch is passed to
    // each preview request and then returned by the previewer. it is used to prevent repeated
    // resubmission when there are no changes.
    epoch: u64,
    area: Area,
    boundary_chars: BoundaryChars,
    ellipsis: char,
}

impl<P> PreviewPane<P> {
    pub(crate) fn new(config: &PreviewConfig, chars: &PickerChars, previewer: P) -> Self {
        Self {
            cache: config
                .cache_size
                .map_or_else(LruCache::unbounded, LruCache::new),
            previewer,
            last_item: None,
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

    /// Update the current preview by polling and resubmitting requests and keeping track if a
    /// redraw is required.
    fn update_current<T: Send + Sync + 'static>(
        &mut self,
        selected: Option<u32>,
        snapshot: &nucleo::Snapshot<T>,
        deadline: Instant,
    ) -> Result<(), P::AbortErr>
    where
        P: Preview<T>,
    {
        if self.last_item != selected {
            self.last_item = selected;
            self.epoch = self.epoch.wrapping_add(1);
            self.pending_redraw = true;
        }
        let Some(idx) = selected else {
            return Ok(());
        };

        let Some(cached) = self.cache.get_mut(&idx) else {
            let evicted = self.cache.push(
                idx,
                Cached {
                    scroll_position: 0,
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
                idx,
                buffer,
                self.epoch,
                deadline,
            )?;
            self.pending_redraw |= matches!(state, RequestState::Ready(_));
            self.cache.peek_mut(&idx).unwrap().state = Some(state);
            return Ok(());
        };
        if matches!(cached.state, Some(RequestState::Ready(_))) {
            return Ok(());
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
            Some(state) => match state.try_into_buffer() {
                Ok(buffer) => Ok(RequestState::Ready(buffer)),
                Err(BufferNotReady::Queued(RequestState::Pending(pending)))
                    if pending.epoch != self.epoch =>
                {
                    pending.reprioritize()
                }
                Err(BufferNotReady::Queued(state) | BufferNotReady::Active(state)) => Ok(state),
                Err(BufferNotReady::Dropped(buffer)) => Err(buffer),
            },
            None => Err(PreviewBuffer::new()),
        };
        let state = match state {
            Ok(state) => state,
            Err(buffer) => submit(
                &mut self.previewer,
                snapshot,
                idx,
                buffer,
                self.epoch,
                deadline,
            )?,
        };
        self.pending_redraw |= matches!(state, RequestState::Ready(_));
        cached.state = Some(state);
        Ok(())
    }
}

impl<P> PreviewPane<P> {
    fn handle_event(&mut self, idx: Option<u32>, event: PreviewEvent) {
        let Some(cached) = idx.and_then(|idx| self.cache.get_mut(&idx)) else {
            return;
        };
        if !matches!(cached.state, Some(RequestState::Ready(_))) {
            return;
        }
        self.pending_redraw |= match event {
            PreviewEvent::ToggleLineNumbers => {
                cached.line_numbers_override = Some(!cached.line_numbers());
                true
            }
            PreviewEvent::SetLineNumbers(enabled) => {
                let previous = cached.line_numbers();
                cached.line_numbers_override = enabled;
                previous != cached.line_numbers()
            }
            event if !self.area.is_empty() => {
                cached.scroll(event, self.area.height.saturating_sub(2))
            }
            _ => false,
        };
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

impl<E, P> Component<E> for PreviewPane<P> {
    fn resize(&mut self, area: Area, _engine: &E) {
        self.resize_area(area);
    }
    fn draw<D: Rect>(&mut self, _engine: &E, rect: &mut D) -> io::Result<()> {
        let preview = self.last_item.and_then(|idx| self.cache.peek(&idx));
        let line_numbers = preview.is_some_and(Cached::line_numbers);
        super::draw::draw(
            rect,
            preview,
            self.boundary_chars,
            self.ellipsis,
            line_numbers,
        )
    }
}

impl<T: Send + Sync + 'static, P: Preview<T>> PreviewComponent<T> for PreviewPane<P> {
    type Error = P::AbortErr;
    const ENABLED: bool = true;
    fn handle(&mut self, event: PreviewEvent, selected_id: Option<u32>) {
        self.handle_event(selected_id, event);
    }
    fn update(
        &mut self,
        selected: Option<u32>,
        snapshot: &nucleo::Snapshot<T>,
        deadline: Instant,
    ) -> Result<bool, P::AbortErr> {
        self.update_current(selected, snapshot, deadline)?;
        Ok(std::mem::take(&mut self.pending_redraw))
    }
    fn restart(&mut self) {
        self.restart_cache();
    }
}

fn submit<T: Send + Sync + 'static, P: Preview<T>>(
    previewer: &mut P,
    snapshot: &nucleo::Snapshot<T>,
    idx: u32,
    mut buffer: PreviewBuffer,
    epoch: u64,
    deadline: Instant,
) -> Result<RequestState, P::AbortErr> {
    buffer.clear();
    let request = PreviewRequest {
        buffer,
        epoch,
        snapshot,
        idx,
    };
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(2));
    previewer
        .preview(request.item(), request, timeout)
        .map(|response| match response {
            PreviewResponse::Ready(buffer) => RequestState::Ready(buffer),
            PreviewResponse::Pending(pending) => RequestState::Pending(pending),
        })
}

#[cfg(test)]
mod tests;
