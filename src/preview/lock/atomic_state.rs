use std::sync::atomic::{AtomicI8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i8)]
pub(super) enum State {
    Queued = 0,
    Active = 1,
    Submitting = 2,
    Complete = 3,
    Cancelled = 4,
    Dropped = 5,
}

pub(super) struct AtomicState(AtomicI8);

impl AtomicState {
    pub(super) const fn new(state: State) -> Self {
        Self(AtomicI8::new(state as i8))
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        Self::from_raw(self.0.load(order))
    }

    pub(super) fn store(&self, state: State, order: Ordering) {
        self.0.store(state as i8, order);
    }

    pub(super) fn compare_exchange(
        &self,
        current: State,
        new: State,
        success: Ordering,
        failure: Ordering,
    ) -> Result<State, State> {
        self.0
            .compare_exchange(current as i8, new as i8, success, failure)
            .map(Self::from_raw)
            .map_err(Self::from_raw)
    }

    fn from_raw(state: i8) -> State {
        // SAFETY: the inner atomic is private and is only initialized and
        // modified through methods which accept a valid State.
        unsafe { std::mem::transmute(state) }
    }
}
