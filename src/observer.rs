//! # An observer channel
use parking_lot::{Condvar, Mutex};
use std::{
    sync::{
        Arc,
        mpsc::{RecvError, RecvTimeoutError, SendError, TryRecvError},
    },
    time::Duration,
};

type Channel<T> = Mutex<(Option<T>, bool)>;

/// The 'notify' end of the single slot channel.
pub(crate) struct Notifier<T> {
    inner: Arc<(Channel<T>, Condvar)>,
}

#[inline]
fn channel_inner<T>(msg: Option<T>) -> (Notifier<T>, Observer<T>) {
    let inner = Arc::new((Mutex::new((msg, true)), Condvar::new()));

    let observer = Observer {
        inner: Arc::clone(&inner),
    };

    let notifier = Notifier { inner };

    (notifier, observer)
}

pub(crate) fn occupied_channel<T>(msg: T) -> (Notifier<T>, Observer<T>) {
    channel_inner(Some(msg))
}

pub(crate) fn channel<T>() -> (Notifier<T>, Observer<T>) {
    channel_inner(None)
}

impl<T> Notifier<T> {
    /// Push a message to the channel. This overwrites any pre-existing message already in the
    /// channel.
    pub fn push(&self, msg: T) -> Result<(), SendError<T>> {
        // in principle, this is a TOCTOU error, since the observer could drop in between this
        // check and the time that the message is pushed to the channel. however, this
        // is not a serious concern here because this is indistinguishable from the case that
        // the injector is dropped immediately (without sending any elements) even if it was
        // successfully received
        //
        // this check should be thought of as 'best-effort', instead of actually being critical
        // for logic.
        if Arc::strong_count(&self.inner) == 1 {
            // there are no observers so the channel is disconnected
            Err(SendError(msg))
        } else {
            // overwrite the channel with the new message and notify an observer that a message
            // is available
            let (lock, cvar) = &*self.inner;

            let previous = {
                let mut channel = lock.lock();
                let previous = channel.0.replace(msg);
                if previous.is_none() {
                    // only notify if we went from unoccupied -> occupied
                    cvar.notify_one();
                }
                previous
            };
            // drop outside the mutex in case of slow / blocking drop
            drop(previous);
            Ok(())
        }
    }
}

impl<T> Drop for Notifier<T> {
    fn drop(&mut self) {
        // when we drop the notifier, we need to inform all observers that are potentially waiting
        // for a message that the channel is closed
        let (lock, cvar) = &*self.inner;

        let mut channel = lock.lock();
        channel.1 = false;
        cvar.notify_all();
    }
}

/// An `Observer` watching for a single message `T`.
///
/// This is similar to the 'receiver' end of a channel of length 1, but instead of blocking, the
/// 'sender' always overwrites any element in the channel. In particular, any message obtained by
/// [`recv`](Observer::recv) or [`try_recv`](Observer::try_recv) is guaranteed to be the most
/// up-to-date at the moment when the message is received.
///
/// # Clone caveats
///
/// The channel may be updated when not observed. Receiving a message **moves it out of the observer**.
/// While an observer can be cheaply cloned (a single [`Arc::clone`]) in order to watch for the message
/// simultaneously from different threads, this means that each update is received *by at most one
/// clone*, instead of independently by each observer.
///
/// For injector observers, this means that the subscribing thread is responsible for sharing the
/// new injector among other threads or defining a custom `Observer` wrapper to manage this
/// synchronization manually.
///
/// For a non-consuming alternative, consider using [`try_peek`](Self::try_peek). However,
/// keep in mind that:
///
/// - repeated peeks do not distinguish between old and new items
/// - the original item may remain in the channel until it is drained
///
/// In the latter case, the [channel can be manually cleared](Observer::clear).
pub struct Observer<T> {
    inner: Arc<(Channel<T>, Condvar)>,
}

impl<T> Clone for Observer<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Observer<T> {
    /// Receive a message, blocking until a message is available or the channel
    /// disconnects.
    pub fn recv(&self) -> Result<T, RecvError> {
        let (lock, cvar) = &*self.inner;
        let mut channel = lock.lock();
        // we need the `none` check since observers can be cloned and another observer could
        // take the item out of the channel between the 'wake' and mutex acquisition
        cvar.wait_while(&mut channel, |channel| channel.0.is_none() && channel.1);
        channel.0.take().ok_or(RecvError)
    }

    /// Receive a message, blocking for at most the timeout until a message is available or the
    /// channel disconnects.
    ///
    /// This operation will fail if no message is received within the given timeout or if there are
    /// no remaining senders.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        let (lock, cvar) = &*self.inner;
        let mut channel = lock.lock();

        cvar.wait_while_for(
            &mut channel,
            |channel| channel.0.is_none() && channel.1,
            timeout,
        );
        match channel.0.take() {
            Some(msg) => Ok(msg),
            None if channel.1 => Err(RecvTimeoutError::Timeout),
            None => Err(RecvTimeoutError::Disconnected),
        }
    }

    /// Optimistically receive a message if one is available without blocking the current thread.
    ///
    /// This operation will fail if there is no message or if there are no remaining senders.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let (lock, _) = &*self.inner;
        let mut channel = lock.lock();
        channel.0.take().ok_or(if channel.1 {
            TryRecvError::Empty
        } else {
            TryRecvError::Disconnected
        })
    }

    /// Discard any buffered message for all observers.
    ///
    /// Calling this method requires acquiring a mutex and dropping the value (if any).
    /// Note that concurrent publication can populate the slot before this method returns, so calling
    /// this method does not guarantee that the channel is empty when it returns.
    pub fn clear(&self) {
        let previous = {
            let (lock, _) = &*self.inner;
            let mut channel = lock.lock();
            channel.0.take()
        };
        drop(previous);
    }

    /// Clone the current message without consuming it or waiting for an update.
    ///
    /// This is the same as [`try_recv`](Self::try_recv), but does not move the message out of the
    /// channel. Read those docs for more details on the channel semantics.
    ///
    /// Note that *cloning blocks the observer*. Use caution for item types `T` which are expensive to clone.
    /// For the common use-case of an [injector observer](crate::Picker::injector_observer), clones
    /// are cheap so this is not a concern.
    ///
    /// This method does no book-keeping to track which item has been cloned. In particular,
    /// repeated calls to this method can return a clone of the same message each time.
    pub fn try_peek(&self) -> Result<T, TryRecvError>
    where
        T: Clone,
    {
        let (lock, _) = &*self.inner;
        let channel = lock.lock();
        channel.0.clone().ok_or(if channel.1 {
            TryRecvError::Empty
        } else {
            TryRecvError::Disconnected
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn clear_empties_shared_slot_and_preserves_connection() {
        let (notifier, observer) = occupied_channel(1);
        let other = observer.clone();

        observer.clear();
        assert_eq!(other.try_recv(), Err(TryRecvError::Empty));
        other.clear();
        assert_eq!(observer.try_peek(), Err(TryRecvError::Empty));

        notifier.push(2).unwrap();
        assert_eq!(other.try_recv(), Ok(2));
    }

    #[test]
    fn clear_discards_buffered_message_after_disconnect() {
        let (notifier, observer) = occupied_channel(1);
        drop(notifier);

        observer.clear();
        assert_eq!(observer.try_recv(), Err(TryRecvError::Disconnected));
        observer.clear();
        assert_eq!(observer.try_peek(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn clear_drops_message_outside_lock() {
        struct OnDrop<'a> {
            observer: Observer<Self>,
            dropped: &'a Cell<bool>,
        }

        impl Drop for OnDrop<'_> {
            fn drop(&mut self) {
                let (lock, _) = &*self.observer.inner;
                let channel = lock
                    .try_lock()
                    .expect("clear must release the channel lock before dropping the message");
                assert!(channel.0.is_none());
                self.dropped.set(true);
            }
        }

        let dropped = Cell::new(false);
        let (notifier, observer) = channel();
        assert!(
            notifier
                .push(OnDrop {
                    observer: observer.clone(),
                    dropped: &dropped,
                })
                .is_ok()
        );

        observer.clear();
        assert!(dropped.get());
    }
}
