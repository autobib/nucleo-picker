use super::{
    BoundaryChars, Preview, PreviewBuffer, PreviewConfig, PreviewEvent, PreviewRequest,
    PreviewResponse,
    cache::{BufferNotReady, Cached, RequestState},
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
    line_numbers: bool,
}

impl<P> PreviewPane<P> {
    pub(crate) fn new(config: &PreviewConfig, chars: &PickerChars, previewer: P) -> Self {
        Self {
            cache: LruCache::new(config.cache_size),
            previewer,
            last_item: None,
            pending_redraw: false,
            epoch: 0,
            area: Area::default(),
            boundary_chars: config.boundary_chars,
            ellipsis: chars.ellipsis,
            line_numbers: config.line_numbers,
        }
    }

    #[cfg(test)]
    fn cached(&self) -> Option<&Cached> {
        self.last_item.and_then(|idx| self.cache.peek(&idx))
    }

    /// Update the current preview by polling and resubmitting requests and keeping track if a
    /// redraw is required.
    fn update_current<T>(
        &mut self,
        selected: Option<(u32, &T)>,
        deadline: Instant,
    ) -> Result<(), P::AbortErr>
    where
        P: Preview<T>,
    {
        let idx = selected.map(|(idx, _)| idx);
        if self.last_item != idx {
            self.last_item = idx;
            self.epoch = self.epoch.wrapping_add(1);
            self.pending_redraw = true;
        }
        let Some((idx, item)) = selected else {
            return Ok(());
        };

        let Some(cached) = self.cache.get_mut(&idx) else {
            let evicted = self.cache.push(
                idx,
                Cached {
                    scroll_position: 0,
                    state: None,
                },
            );
            let buffer = evicted
                .and_then(|(_, mut cached)| cached.state.take())
                .map_or_else(PreviewBuffer::new, RequestState::into_buffer);
            let state = submit(&mut self.previewer, item, buffer, self.epoch, deadline)?;
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
            Err(buffer) => submit(&mut self.previewer, item, buffer, self.epoch, deadline)?,
        };
        self.pending_redraw |= matches!(state, RequestState::Ready(_));
        cached.state = Some(state);
        Ok(())
    }
}

impl<P> PreviewPane<P> {
    fn scroll(&mut self, idx: Option<u32>, event: PreviewEvent) {
        let height = self.area.height.saturating_sub(2);
        if self.area.is_empty() || height == 0 {
            return;
        }
        self.pending_redraw |= idx
            .and_then(|idx| self.cache.get_mut(&idx))
            .is_some_and(|cached| cached.scroll(event, height));
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
        let preview = self
            .last_item
            .and_then(|idx| self.cache.peek(&idx))
            .and_then(|cached| match &cached.state {
                Some(RequestState::Ready(buffer)) => Some((buffer, cached.scroll_position)),
                _ => None,
            });
        super::draw::draw(
            rect,
            preview,
            self.boundary_chars,
            self.ellipsis,
            self.line_numbers,
        )
    }
}

impl<T, P: Preview<T>> PreviewComponent<T> for PreviewPane<P> {
    type Error = P::AbortErr;
    const ENABLED: bool = true;
    fn handle(&mut self, event: PreviewEvent, selected_id: Option<u32>) {
        self.scroll(selected_id, event);
    }
    fn update(
        &mut self,
        selected: Option<(u32, &T)>,
        deadline: Instant,
    ) -> Result<bool, P::AbortErr> {
        self.update_current(selected, deadline)?;
        Ok(std::mem::take(&mut self.pending_redraw))
    }
    fn restart(&mut self) {
        self.restart_cache();
    }
}

fn submit<T, P: Preview<T>>(
    previewer: &mut P,
    item: &T,
    mut buffer: PreviewBuffer,
    epoch: u64,
    deadline: Instant,
) -> Result<RequestState, P::AbortErr> {
    buffer.clear();
    let request = PreviewRequest { buffer, epoch };
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(2));
    previewer
        .preview(item, request, timeout)
        .map(|response| match response {
            PreviewResponse::Ready(buffer) => RequestState::Ready(buffer),
            PreviewResponse::Pending(pending) => RequestState::Pending(pending),
        })
}

#[cfg(test)]
mod tests;
