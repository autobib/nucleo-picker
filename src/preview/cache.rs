use super::{PendingPreview, PreviewBuffer, lock};

/// A single entry in the preview cache.
pub(crate) struct Cached {
    pub scroll_position: usize,
    pub state: Option<RequestState>,
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
}

/// A preview buffer that is not ready yet.
pub(crate) enum BufferNotReady {
    /// The preview request is waiting to be processed.
    Queued(RequestState),
    /// The preview request is currently being processed.
    Active(RequestState),
    /// The request was dropped.
    Dropped(PreviewBuffer),
}

impl RequestState {
    pub fn into_buffer(self) -> PreviewBuffer {
        match self {
            Self::Ready(buffer) => buffer,
            Self::Pending(pending) => pending.reader.cancel_any(),
        }
    }

    /// Obtain the preview buffer if it is ready, without blocking.
    ///
    /// If the preview is queued or active, the corresponding state is returned in the `Err` variant.
    pub fn try_into_buffer(self) -> Result<PreviewBuffer, BufferNotReady> {
        match self {
            Self::Pending(subscription) => {
                let PendingPreview { epoch, reader } = subscription;
                match reader.poll() {
                    lock::Poll::Ready(buffer) => Ok(buffer),
                    lock::Poll::Queued(reader) => {
                        Err(BufferNotReady::Queued(Self::Pending(PendingPreview {
                            reader,
                            epoch,
                        })))
                    }
                    lock::Poll::Active(reader) => {
                        Err(BufferNotReady::Active(Self::Pending(PendingPreview {
                            reader,
                            epoch,
                        })))
                    }
                    lock::Poll::Dropped(buffer) => Err(BufferNotReady::Dropped(buffer)),
                }
            }
            Self::Ready(buffer) => Ok(buffer),
        }
    }
}

impl PendingPreview {
    /// Attempt to cancel a queued request in order to later resubmit it (for higher priority).
    ///
    /// If the buffer is in fact ready, or a worker is currently preparing the buffer, this is
    /// returned in the `Ok` variant through [`RequestState`]. Otherwise, the buffer is recovered and
    /// returned in the `Err` variant.
    pub(crate) fn reprioritize(self) -> Result<RequestState, PreviewBuffer> {
        let Self { reader, epoch } = self;
        match reader.cancel_queued() {
            lock::CancelQueued::Cancelled(buffer) | lock::CancelQueued::Dropped(buffer) => {
                Err(buffer)
            }
            lock::CancelQueued::Active(reader) => Ok(RequestState::Pending(Self { reader, epoch })),
            lock::CancelQueued::Ready(buffer) => Ok(RequestState::Ready(buffer)),
        }
    }
}
