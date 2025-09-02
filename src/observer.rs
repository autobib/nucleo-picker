//! # An observer channel
use parking_lot::{Condvar, Mutex};
use std::sync::{
    Arc,
    mpsc::{RecvError, SendError, TryRecvError},
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
        if Arc::strong_count(&self.inner) == 1 {
            // there are no senders so the channel is disconnected
            Err(SendError(msg))
        } else {
            // overwrite the channel with the new message and notify an observer that a message
            // is avaliable
            let (lock, cvar) = &*self.inner;
            let mut channel = lock.lock();
            channel.0 = Some(msg);
            cvar.notify_one();
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
/// The channel may be updated when not observed. Receiving a message moves it out of the observer.
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
        match channel.0.take() {
            Some(msg) => Ok(msg),
            None => {
                if channel.1 {
                    // the channel is active, so we wait for a notification
                    cvar.wait(&mut channel);

                    // we received a notification that there was a change
                    match channel.0.take() {
                        // the change was that a new message has been pushed, so we can return it
                        Some(msg) => Ok(msg),
                        // there is no message despite the notification, so the channel is
                        // disconnected. this path is followed if the notifier is dropped while we
                        // are waiting for a new message
                        None => Err(RecvError),
                    }
                } else {
                    Err(RecvError)
                }
            }
        }
    }

    /// Optimistically receive a message if one is available without blocking the current thread.
    ///
    /// This operation will fail if there is no message or if there are are no remaining senders.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let (lock, _) = &*self.inner;
        let mut channel = lock.lock();
        channel.0.take().ok_or(if channel.1 {
            TryRecvError::Empty
        } else {
            TryRecvError::Disconnected
        })
    }
}
