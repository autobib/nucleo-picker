use super::{PreviewBuffer, lock, request::PendingPreview};

/// A single entry in the preview cache.
pub(crate) struct Cached {
    pub scroll_position: usize,
    pub horizontal_position: usize,
    pub line_numbers_override: Option<bool>,
    pub state: Option<RequestState>,
}

impl Cached {
    pub fn number_width(&self, width: u16) -> u16 {
        let Some(RequestState::Ready(buffer)) = &self.state else {
            return 0;
        };
        if !self.line_numbers() {
            return 0;
        }
        let number_width = buffer.lines().len().ilog10() as u16 + 2;
        if number_width < width.saturating_sub(1) {
            number_width
        } else {
            0
        }
    }

    pub fn line_numbers(&self) -> bool {
        self.line_numbers_override
            .unwrap_or_else(|| match &self.state {
                Some(RequestState::Ready(buffer)) => buffer.line_numbers(),
                _ => false,
            })
    }
}

impl Drop for Cached {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            drop(state.into_buffer());
        }
    }
}

/// The status of a preview request.
pub(crate) enum RequestState {
    Pending(PendingPreview),
    Ready(PreviewBuffer),
    Retry(PreviewBuffer),
}

impl RequestState {
    pub fn into_buffer(self) -> PreviewBuffer {
        match self {
            Self::Ready(buffer) | Self::Retry(buffer) => buffer,
            Self::Pending(pending) => pending.reader.cancel_any(),
        }
    }

    /// Check for state updates of this request without blocking.
    pub fn poll(self, current_epoch: u64) -> Self {
        let Self::Pending(PendingPreview { epoch, reader }) = self else {
            return self;
        };
        match reader.poll() {
            lock::Poll::Ready(buffer) => Self::Ready(buffer),
            lock::Poll::Queued(reader) if epoch != current_epoch => {
                PendingPreview { reader, epoch }.reprioritize()
            }
            lock::Poll::Queued(reader) | lock::Poll::Active(reader) => {
                Self::Pending(PendingPreview { reader, epoch })
            }
            lock::Poll::Dropped(buffer) => Self::Retry(buffer),
        }
    }
}

impl PendingPreview {
    /// Attempt to cancel a queued request in order to later resubmit it (for higher priority).
    fn reprioritize(self) -> RequestState {
        let Self { reader, epoch } = self;
        match reader.cancel_queued() {
            lock::CancelQueued::Cancelled(buffer) | lock::CancelQueued::Dropped(buffer) => {
                RequestState::Retry(buffer)
            }
            lock::CancelQueued::Active(reader) => RequestState::Pending(Self { reader, epoch }),
            lock::CancelQueued::Ready(buffer) => RequestState::Ready(buffer),
        }
    }
}

#[cfg(test)]
mod tests;
