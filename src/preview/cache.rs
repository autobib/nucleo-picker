use super::{
    Buffer, Pane, lock,
    request::{Pending, Response},
};

/// A single entry in the preview cache.
#[derive(Default)]
pub(crate) struct Cached {
    pub state: Option<RequestState>,
}

impl Cached {
    pub fn into_buffer(mut self) -> Buffer {
        self.state
            .take()
            .map_or_else(Buffer::new, RequestState::into_buffer)
    }

    #[cfg(test)]
    pub fn pane(&self) -> Option<&Pane> {
        match &self.state {
            Some(RequestState::Ready(pane)) => Some(pane),
            _ => None,
        }
    }

    pub fn pane_mut(&mut self) -> Option<&mut Pane> {
        match &mut self.state {
            Some(RequestState::Ready(pane)) => Some(pane),
            _ => None,
        }
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
    Pending(Pending),
    Active(Pending),
    Ready(Pane),
    Retry(Buffer),
}

impl RequestState {
    pub fn into_buffer(self) -> Buffer {
        match self {
            Self::Ready(pane) => pane.into_buffer(),
            Self::Retry(buffer) => buffer,
            Self::Pending(pending) | Self::Active(pending) => {
                pending.reader.cancel_any().into_buffer()
            }
        }
    }

    /// Check for state updates of this request without blocking.
    pub fn poll(self) -> Self {
        let (Self::Pending(pending) | Self::Active(pending)) = self else {
            return self;
        };
        let Pending { epoch, reader } = pending;
        match reader.poll() {
            lock::Poll::Ready(pane) => Self::Ready(pane),
            lock::Poll::Queued(reader) => Self::Pending(Pending { reader, epoch }),
            lock::Poll::Active(reader) => Self::Active(Pending { reader, epoch }),
            lock::Poll::Dropped(pane) => Self::Retry(pane.into_buffer()),
        }
    }

    pub fn reprioritize(self, current_epoch: u64) -> Self {
        match self {
            Self::Pending(pending) if pending.epoch != current_epoch => pending.reprioritize(),
            state => state,
        }
    }

    pub fn cancel_queued(self) -> Self {
        match self {
            Self::Pending(pending) => pending.reprioritize(),
            state => state,
        }
    }
}

impl From<Response> for RequestState {
    fn from(response: Response) -> Self {
        match response {
            Response::Ready(pane) => Self::Ready(pane),
            Response::Pending(pending) => Self::Pending(pending),
        }
    }
}

impl Pending {
    /// Attempt to cancel a queued request in order to later resubmit it (for higher priority).
    fn reprioritize(self) -> RequestState {
        let Self { reader, epoch } = self;
        match reader.cancel_queued() {
            lock::CancelQueued::Cancelled(pane) | lock::CancelQueued::Dropped(pane) => {
                RequestState::Retry(pane.into_buffer())
            }
            lock::CancelQueued::Active(reader) => RequestState::Active(Self { reader, epoch }),
            lock::CancelQueued::Ready(pane) => RequestState::Ready(pane),
        }
    }
}

#[cfg(test)]
mod tests;
