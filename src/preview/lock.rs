//! # A single-initialization data wrapper with cancellation.
//!
//! This module implements a single-initialization type with cancellation. The memory location is
//! referred to as a slot, and is initialized with data `T`. There are two accessors: a reader and
//! a writer. The reader is waiting for the writer to submit a completed value. While waiting, the
//! reader may cancel the request to reclaim the internal buffer.
//!
//! The initial state of the slot is [queued](State::Queued). In this state, the data is not yet initialized, and
//! both the reader and writer are waiting. When the writer is ready to begin work, it becomes
//! [active](State::Active). An active writer does not have access to the value in the slot, so the
//! reader can still cancel the request and reclaim that value. Once the writer has produced a
//! value, it briefly enters the private [submitting](State::Submitting) state while swapping the
//! values, then publishes the [complete](State::Complete) state.

mod atomic_state;

use std::{
    cell::UnsafeCell,
    hint::{spin_loop, unreachable_unchecked},
    mem::ManuallyDrop,
    sync::{Arc, atomic::Ordering},
};

use atomic_state::{AtomicState, State};

/// The internal shared state.
struct Inner<T> {
    state: AtomicState,
    data: UnsafeCell<Option<T>>,
}

// `T` is never accessed concurrently since the atomic state transfers exclusive
// access between the two handles.
unsafe impl<T: Send> Send for Inner<T> {}
unsafe impl<T: Send> Sync for Inner<T> {}

/// Initialize a new request with the reader and writer, wrapping the provided data.
pub fn request<T>(data: T) -> (Reader<T>, QueuedWriter<T>) {
    let inner = Arc::new(Inner::new(data));
    (
        Reader {
            inner: Arc::clone(&inner),
        },
        QueuedWriter { inner },
    )
}

impl<T> Inner<T> {
    fn new(data: T) -> Self {
        Self {
            state: AtomicState::new(State::Queued),
            data: UnsafeCell::new(Some(data)),
        }
    }

    // SAFETY: the caller must be the unique owner of the value in the state
    // machine and must arrange for this to be called exactly once.
    unsafe fn take_data(&self) -> T {
        unsafe { (&mut *self.data.get()).take().unwrap_unchecked() }
    }
}

/// The reader side of a request.
pub struct Reader<T> {
    inner: Arc<Inner<T>>,
}

/// A writer which has not started modifying the data.
pub struct QueuedWriter<T> {
    inner: Arc<Inner<T>>,
}

/// A writer which has started work and owns the right to submit a value.
pub struct ActiveWriter<T> {
    inner: Arc<Inner<T>>,
}

/// The result of polling a request.
#[must_use]
pub enum Poll<T> {
    Ready(T),
    Queued(Reader<T>),
    Active(Reader<T>),
    Dropped(T),
}

/// The result of attempting to cancel a queued request.
#[must_use]
pub enum CancelQueued<T> {
    Cancelled(T),
    Active(Reader<T>),
    Ready(T),
    Dropped(T),
}

impl<T> Reader<T> {
    /// Cancels a request if it is still queued.
    pub fn cancel_queued(self) -> CancelQueued<T> {
        let inner = self.inner;

        match inner.state.compare_exchange(
            State::Queued,
            State::Cancelled,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // A queued writer cannot access the slot.
                let data = unsafe { inner.take_data() };
                CancelQueued::Cancelled(data)
            }
            Err(State::Active) => CancelQueued::Active(Self { inner }),
            Err(mut state @ State::Submitting) => {
                while state == State::Submitting {
                    spin_loop();
                    state = inner.state.load(Ordering::Acquire);
                }
                assert_eq!(state, State::Complete, "invalid submitting state");
                let data = unsafe { inner.take_data() };
                CancelQueued::Ready(data)
            }
            Err(State::Complete) => {
                let data = unsafe { inner.take_data() };
                CancelQueued::Ready(data)
            }
            Err(State::Dropped) => {
                let data = unsafe { inner.take_data() };
                CancelQueued::Dropped(data)
            }
            Err(state) => {
                debug_assert!(false, "invalid reader state {state:?}");
                // SAFETY: the state machine permits no other transition from a live Reader.
                unsafe { unreachable_unchecked() }
            }
        }
    }

    /// Reclaim the data from the request.
    pub fn cancel_any(self) -> T {
        let reader = match self.cancel_queued() {
            CancelQueued::Cancelled(data)
            | CancelQueued::Ready(data)
            | CancelQueued::Dropped(data) => return data,
            CancelQueued::Active(reader) => reader,
        };

        match reader.inner.state.compare_exchange(
            State::Active,
            State::Cancelled,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // An active writer does not have access to the slot.
                unsafe { reader.inner.take_data() }
            }
            Err(mut state @ State::Submitting) => {
                while state == State::Submitting {
                    spin_loop();
                    state = reader.inner.state.load(Ordering::Acquire);
                }
                assert_eq!(state, State::Complete, "invalid submitting state");
                unsafe { reader.inner.take_data() }
            }
            Err(State::Complete | State::Dropped) => unsafe { reader.inner.take_data() },
            Err(state) => {
                debug_assert!(false, "invalid reader state {state:?}");
                // SAFETY: the state machine permits no other transition from an active request.
                unsafe { unreachable_unchecked() }
            }
        }
    }

    /// Check if the data is ready without blocking.
    pub fn poll(self) -> Poll<T> {
        let inner = self.inner;
        match inner.state.load(Ordering::Acquire) {
            State::Queued => Poll::Queued(Self { inner }),
            State::Active => Poll::Active(Self { inner }),
            State::Submitting => {
                let mut state = State::Submitting;
                while state == State::Submitting {
                    spin_loop();
                    state = inner.state.load(Ordering::Acquire);
                }
                debug_assert_eq!(state, State::Complete);
                let data = unsafe { inner.take_data() };
                Poll::Ready(data)
            }
            State::Complete => {
                // The Acquire load pairs with the writer's Release transition.
                let data = unsafe { inner.take_data() };
                Poll::Ready(data)
            }
            State::Dropped => {
                let data = unsafe { inner.take_data() };
                Poll::Dropped(data)
            }
            State::Cancelled => {
                debug_assert!(false, "a live Reader cannot observe Cancelled");
                // SAFETY: cancellation consumes the only Reader.
                unsafe { unreachable_unchecked() }
            }
        }
    }
}

impl<T> QueuedWriter<T> {
    /// Returns whether the reader has cancelled this request.
    ///
    /// A `false` result is only advisory; cancellation may happen before the
    /// writer attempts to become active.
    pub fn is_cancelled(&self) -> bool {
        self.inner.state.load(Ordering::Acquire) == State::Cancelled
    }

    /// Declare that this writer is actively writing data.
    ///
    /// Calling this method hints to the reader that cancellation may not be required. Note that
    /// this method will fail and return `None` if the reader has already cancelled the request.
    pub fn start(self) -> Option<ActiveWriter<T>> {
        let inner = self.into_inner();

        match inner.state.compare_exchange(
            State::Queued,
            State::Active,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => Some(ActiveWriter { inner }),
            Err(State::Cancelled) => None,
            Err(x) => {
                debug_assert!(false, "invalid queued-writer state {x:?}");
                // SAFETY: a QueuedWriter can only be Queued or concurrently Cancelled.
                unsafe { unreachable_unchecked() }
            }
        }
    }

    fn into_inner(self) -> Arc<Inner<T>> {
        let this = ManuallyDrop::new(self);
        // SAFETY: `this` will not be dropped, so this moves its `Arc` exactly
        // once instead of duplicating it.
        unsafe { std::ptr::read(&this.inner) }
    }
}

impl<T> Drop for QueuedWriter<T> {
    fn drop(&mut self) {
        match self.inner.state.compare_exchange(
            State::Queued,
            State::Dropped,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {}
            // A concurrent cancellation gives ownership to the reader.
            Err(State::Cancelled) => {}
            Err(x) => {
                debug_assert!(false, "invalid queued-writer state {x:?}");
                // SAFETY: a QueuedWriter can only be Queued or concurrently Cancelled.
                unsafe { unreachable_unchecked() }
            }
        }
    }
}

impl<T> ActiveWriter<T> {
    /// Publish a completed value.
    ///
    /// The caller is not provided mutable data to the buffer internal to this writer: instead, the
    /// data must be swapped in.
    ///
    /// This method may return `false` if the request was cancelled, in which case `data` is
    /// guaranteed to be unmodified. If this method returns `true`, the data handle has be
    /// overwritten by the data internal to the slot.
    #[must_use = "publication may fail if the request was cancelled"]
    pub fn publish(self, data: &mut T) -> bool {
        let inner = self.into_inner();

        match inner.state.compare_exchange(
            State::Active,
            State::Submitting,
            Ordering::Acquire,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // SAFETY: Submitting excludes the reader from taking the slot.
                // No user code runs before the state is changed to Complete.
                unsafe {
                    std::mem::swap((&mut *inner.data.get()).as_mut().unwrap_unchecked(), data);
                }
                inner.state.store(State::Complete, Ordering::Release);
                true
            }
            Err(State::Cancelled) => false,
            Err(x) => {
                debug_assert!(false, "invalid active-writer state {x:?}");
                // SAFETY: an ActiveWriter can only be Active or concurrently Cancelled.
                unsafe { unreachable_unchecked() }
            }
        }
    }

    /// Returns whether the reader has cancelled this request.
    pub fn is_cancelled(&self) -> bool {
        self.inner.state.load(Ordering::Acquire) == State::Cancelled
    }

    fn into_inner(self) -> Arc<Inner<T>> {
        let this = ManuallyDrop::new(self);
        // SAFETY: `this` will not be dropped, so this moves its `Arc` exactly
        // once instead of duplicating it.
        unsafe { std::ptr::read(&this.inner) }
    }
}

impl<T> Drop for ActiveWriter<T> {
    fn drop(&mut self) {
        match self.inner.state.compare_exchange(
            State::Active,
            State::Dropped,
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            Ok(_) => {}
            Err(State::Cancelled) => {}
            Err(x) => {
                debug_assert!(false, "invalid active-writer state {x:?}");
                // SAFETY: an ActiveWriter can only be Active or concurrently Cancelled.
                unsafe { unreachable_unchecked() }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;

    struct Counted {
        _value: String,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            self.drops.fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    fn counted(value: &str) -> (Counted, Arc<AtomicUsize>) {
        let drops = Arc::new(AtomicUsize::new(0));
        (
            Counted {
                _value: value.into(),
                drops: Arc::clone(&drops),
            },
            drops,
        )
    }

    #[test]
    fn publish_swaps_values() {
        let (reader, writer) = request(String::from("reusable"));
        let active = writer.start().unwrap();
        let mut completed = String::from("done");
        assert!(active.publish(&mut completed));

        let Poll::Ready(data) = reader.poll() else {
            panic!("published request was not ready");
        };
        assert_eq!(data, "done");
        assert_eq!(completed, "reusable");
    }

    #[test]
    fn taking_data_leaves_an_empty_slot() {
        let (data, drops) = counted("original");
        let (reader, writer) = request(data);
        let (mut completed, completed_drops) = counted("completed");
        assert!(writer.start().unwrap().publish(&mut completed));

        let Poll::Ready(data) = reader.poll() else {
            panic!("published request was not ready");
        };
        assert_eq!(drops.load(AtomicOrdering::Relaxed), 0);
        assert_eq!(completed_drops.load(AtomicOrdering::Relaxed), 0);
        drop(data);
        assert_eq!(completed_drops.load(AtomicOrdering::Relaxed), 1);
        drop(completed);
        assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
    }

    #[test]
    fn cancel_queued_returns_data_and_prevents_activation() {
        let (reader, writer) = request(String::from("queued"));
        assert!(!writer.is_cancelled());
        let CancelQueued::Cancelled(data) = reader.cancel_queued() else {
            panic!("queued cancellation did not succeed");
        };
        assert_eq!(data, "queued");
        assert!(writer.is_cancelled());
        assert!(writer.start().is_none());
    }

    #[test]
    fn cancel_active_reclaims_data_and_prevents_publication() {
        let (reader, writer) = request(String::from("reusable"));
        let active = writer.start().unwrap();
        assert_eq!(reader.cancel_any(), "reusable");
        assert!(active.is_cancelled());

        let mut completed = String::from("done");
        assert!(!active.publish(&mut completed));
        assert_eq!(completed, "done");
    }

    #[test]
    fn cancel_queued_does_not_cancel_active() {
        let (reader, writer) = request(String::from("reusable"));
        let active = writer.start().unwrap();
        let CancelQueued::Active(reader) = reader.cancel_queued() else {
            panic!("active request was not reported as active");
        };

        let mut completed = String::from("done");
        assert!(active.publish(&mut completed));
        let Poll::Ready(data) = reader.poll() else {
            panic!("published request was not ready");
        };
        assert_eq!(data, "done");
        assert_eq!(completed, "reusable");
    }

    #[test]
    fn dropped_writers_relinquish_data() {
        let (reader, writer) = request(String::from("queued drop"));
        drop(writer);
        let Poll::Dropped(data) = reader.poll() else {
            panic!("dropped request was not reported as dropped");
        };
        assert_eq!(data, "queued drop");

        let (reader, writer) = request(String::from("active drop"));
        let active = writer.start().unwrap();
        drop(active);
        let Poll::Dropped(data) = reader.poll() else {
            panic!("dropped request was not reported as dropped");
        };
        assert_eq!(data, "active drop");
    }

    #[test]
    fn reader_drop_while_queued_leaves_writer_usable() {
        for active in [false, true] {
            let (data, drops) = counted("queued");
            let (reader, writer) = request(data);
            drop(reader);

            if active {
                drop(writer.start().unwrap());
            } else {
                drop(writer);
            }

            assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        }
    }

    #[test]
    fn reader_drop_after_writer_exit_destroys_data() {
        for active in [false, true] {
            let (data, drops) = counted("data");
            let (reader, writer) = request(data);

            if active {
                drop(writer.start().unwrap());
            } else {
                drop(writer);
            }

            assert_eq!(drops.load(AtomicOrdering::Relaxed), 0);
            drop(reader);
            assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        }
    }

    #[test]
    fn reader_and_writer_drop_race_destroys_once() {
        for active_first in [false, true] {
            for _ in 0..1_000 {
                let (data, drops) = counted("data");
                let (reader, writer) = request(data);

                thread::scope(|scope| {
                    scope.spawn(move || drop(reader));
                    scope.spawn(move || {
                        if active_first {
                            drop(writer.start());
                        } else {
                            drop(writer);
                        }
                    });
                });

                assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
            }
        }
    }

    #[test]
    fn cancel_and_activate_race_has_one_owner() {
        for _ in 0..1_000 {
            let (data, drops) = counted("data");
            let (reader, writer) = request(data);

            thread::scope(|scope| {
                scope.spawn(move || drop(reader.cancel_any()));
                scope.spawn(move || drop(writer.start()));
            });

            assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        }
    }

    #[test]
    fn cancel_and_publish_race_has_one_slot_owner() {
        for _ in 0..1_000 {
            let (reader, writer) = request(String::from("reusable"));
            let active = writer.start().unwrap();

            thread::scope(|scope| {
                scope.spawn(move || {
                    let data = reader.cancel_any();
                    assert!(data == "reusable" || data == "completed");
                });
                scope.spawn(move || {
                    let mut data = String::from("completed");
                    if active.publish(&mut data) {
                        assert_eq!(data, "reusable");
                    } else {
                        assert_eq!(data, "completed");
                    }
                });
            });
        }
    }

    #[test]
    fn handles_are_send_when_data_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Reader<std::cell::Cell<u8>>>();
        assert_send::<QueuedWriter<std::cell::Cell<u8>>>();
        assert_send::<ActiveWriter<std::cell::Cell<u8>>>();
    }
}
