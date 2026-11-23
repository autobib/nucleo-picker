//! # Previewer request types
//!
//! This module contains the core types used to coordinate request state between a picker and a
//! (asynchronous) previewer. This is mainly relevant if you are designing a custom
//! [`Preview`](super::Preview) implementation.
//!
//! ## Overview
//!
//! 1. The basic type is the [`Request`]. This request is received when the preview method is
//!    called. There are two options: handle the request immediately with
//!    [`Request::ready`], or handle the request later with [`Request::defer`].
//! 2. If you defer a request, you get a [`QueuedRequest`]. This represents a request which
//!    you will work on later. When you are ready to process the request, call [`QueuedRequest::start`].
//! 3. After starting work on a queued request, you get an [`ActiveRequest`]. Publish the
//!    preview buffer when finished with [`ActiveRequest::publish`].
//!
//! ## Request coordination with the picker
//!
//! The [`Request`] is sent by the picker when it calls the [`preview`](super::Preview::preview) method.
//! This type represents a synchronous request for a preview. If the preview can be immediately
//! satisfied, call [`Request::ready`], fill the preview buffer, and return the preview buffer in
//! the response.
//!
//! If the preview cannot immediately be generated, call [`Request::defer`]. This returns a
//! [`QueuedRequest`] as well as a handle, to be returned to the picker, which is associated with
//! the queued request. The queued request should be sent to another thread to be completed
//! asynchronously so that the picker can continue rendering the frame.
//!
//! A queued request may be cancelled at any time, and the underlying request can be re-submitted.
//! The picker will do this in order to re-prioritize requests when there is a backlog of pending
//! preview requests. When a request is cancelled, the various methods will return a
//! [`CancelledRequest`].
//!
//! When the queued request is ready to be handled, convert to an [`ActiveRequest`] with the
//! [`QueuedRequest::start`] method. This will prevent cancellation for re-prioritization:
//! once this method is called, the picker will wait for this request to be satisfied without
//! requesting it again (unless the request is dropped). Note that the preview request may still be cancelled
//! if the preview is no longer required. When the preview itself is ready, call [`ActiveRequest::publish`]
//! to make the preview available to the picker.
//!
//! When a queued request is very outdated, the picker will cancel the request regardless of its
//! state. When the picker restarts or exits, all open requests will be cancelled before the previewer drops.
//!
//! If the previewer wishes to cancel a given request, it may simply drop the request. The picker
//! will resubmit the request on a subsequent update. Note that repeated drops will cause the
//! preview to display indefinitely as loading: if the request was dropped because it cannot be
//! satisfied, the previewer should instead satisfy the request by publishing an error.
//!
//! ## Prefetch
//!
//! The picker also coordinates pre-emptive ("prefetch") requests with the previewer. Prefetch
//! cancellation logic is handled differently: requests are only retried when the match list or
//! the highlighted item changes. If a prefetched item becomes highlighted (so a normal request is sent),
//! the prefetch request is cancelled if not yet active and upgraded to a normal request.
//!
//! ### Basic example
//!
//! Here is a simple coordination example with a single preview worker at the other end of a MPSC
//! channel.
//!
//! ```
//! use std::sync::mpsc::{Receiver, Sender};
//! use nucleo_picker_core::preview::Pane;
//! use nucleo_picker_interface::preview::request::{QueuedRequest, Request, Response};
//!
//! // Queue a request for completion.
//! fn enqueue(
//!     request: Request<'_, String>,
//!     worker: &Sender<QueuedRequest<String>>,
//! ) -> Response {
//!     let (pending, queued) = request.defer();
//!     worker.send(queued).expect("preview worker disconnected");
//!     Response::Pending(pending)
//! }
//!
//! // Complete the queued preview requests (in a separate thread).
//! fn complete(receiver: Receiver<QueuedRequest<String>>) {
//!     // reusable scratch space for all requests
//!     let mut pane = Pane::new();
//!
//!     while let Ok(queued) = receiver.recv() {
//!         let Ok(request) = queued.start() else {
//!             continue;
//!         };
//!         pane.buffer_mut().push_text(request.item());
//!         if request.publish(&mut pane).is_err() {
//!             pane.clear();
//!         }
//!     }
//! }
//! ```
//!
//! ## Buffer reuse
//!
//! The [`Request`] internally contains a pre-cleared preview pane that may be recycled from a
//! previous preview pane. When publishing a pane through [`ActiveRequest::publish`], the
//! worker should maintain a separate scratch pane and swap it with the request pane on
//! publication. If publication succeeds, the pane will be empty, but if it fails the pane will
//! have the original contents which the worker must clear manually.
//!
//! ## Previewing item and lifetimes
//!
//! The item for which the preview was originally requested is always available using an associated
//! `item` method; see [`Request::item`], [`QueuedRequest::item`], and
//! [`ActiveRequest::item`]. Internally, the deferred request types use atomic reference counting to
//! hold on to the underlying item pool (shared with the picker) as long as any request is
//! not dropped. This means that the items will remain in memory even if the picker shuts down,
//! if there are any remaining requests which have not been dropped.

use std::fmt;

use nucleo_picker_core::{
    preview::{Buffer, Pane},
    session::{ItemId, ItemRef, Matches, OwnedItem},
};

use super::lock::{ActiveWriter, QueuedWriter, Reader, request};

/// The outcome of a preview request.
pub enum Response {
    /// The preview could be generated immediately.
    Ready(Pane),
    /// Preview generation has been deferred.
    Pending(Pending),
}

/// A request to generate a preview.
///
/// This type has a lifetime `'a` attached to the picker, but once the request is [deferred](Self::defer)
/// the lifetime is no longer held.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct Request<'a, T> {
    pub(crate) buffer: Buffer,
    pub(crate) epoch: u64,
    pub(crate) matches: Matches<'a, T>,
    pub(crate) id: ItemId,
}

/// A preview request waiting to be processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct QueuedRequest<T> {
    writer: QueuedWriter<Pane>,
    item: OwnedItem<T>,
}

/// A preview request that is currently being processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct ActiveRequest<T> {
    writer: ActiveWriter<Pane>,
    item: OwnedItem<T>,
}

/// A preview request that was cancelled by the picker.
///
/// This retains the original item and keeps the item pool alive until dropped. See the
/// [module-level documentation](crate::preview::request) for more detail.
pub struct CancelledRequest<T> {
    item: OwnedItem<T>,
}

/// A subscription to a pending preview.
///
/// This is the type which should be returned to the picker if a [`Preview`](super::Preview)
/// implementation wishes to defer preview generation.
pub struct Pending {
    pub(crate) reader: Reader<Pane>,
    pub(crate) epoch: u64,
}

impl<'a, T: Send + Sync + 'static> Request<'a, T> {
    /// Declare that the preview is already complete and obtain a cleared preview pane to return to the picker.
    ///
    /// # Reducing allocations
    ///
    /// It is valid for the caller to allocate a new [`Pane`] and return it
    /// directly, ignoring the preview request. However, it is encouraged to re-use the
    /// [`Pane`] obtained using this method since the pane may be recycled from previous
    /// requests, decreasing allocation overhead.
    pub fn ready(self) -> Pane {
        self.buffer.into_pane()
    }

    /// Defer handling of the preview request.
    ///
    /// The resulting [`Pending`] will be notified when the preview request has been handled,
    /// and it must be returned to the picker. The corresponding [`QueuedRequest`] is used to
    /// complete the preview request when ready.
    pub fn defer(self) -> (Pending, QueuedRequest<T>) {
        let Self {
            buffer,
            epoch,
            matches,
            id,
        } = self;
        // SAFETY: the request index is initialized in this matches, whose borrow prevents restart.
        let item = unsafe { matches.owned_item_unchecked(id) };
        let (reader, writer) = request(buffer.into_pane());
        (Pending { reader, epoch }, QueuedRequest { writer, item })
    }

    /// The item corresponding to the request.
    #[inline]
    pub fn item(&self) -> &'a T {
        self.item_ref().value()
    }

    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// Returns the item corresponding to this request along with its identifier.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'a, T> {
        // SAFETY: the index is guaranteed to be valid for the matches view
        unsafe { self.matches.item_unchecked(self.id) }
    }
}

impl<T: Send + Sync + 'static> QueuedRequest<T> {
    /// Declare that the previewer is ready to start generating the preview for this queued request.
    ///
    /// This method should be called when the request is actively being processed. Calling this
    /// method will prevent the picker from cancelling and resubmitting the preview request in
    /// order to increase priority.
    ///
    /// This method fails if the picker cancelled the request before it could start.
    pub fn start(self) -> Result<ActiveRequest<T>, CancelledRequest<T>> {
        let Self { writer, item } = self;
        match writer.start() {
            Some(writer) => Ok(ActiveRequest { writer, item }),
            None => Err(CancelledRequest { item }),
        }
    }

    /// Check if the request has been cancelled by the picker.
    ///
    /// If this method returns true, this means that the picker has cancelled the request. This
    /// can occur for one of two reasons:
    ///
    /// 1. The picker has decided to increase the priority of this request. This is done by
    ///    cancelling stale requests and calling the [`Preview::preview`](super::Preview::preview) method again with the same
    ///    data.
    /// 2. The preview pane is no longer required by the picker.
    ///
    /// If true, the next call to [`start`](Self::start) will fail. If false, starting may still
    /// fail if cancellation occurs between the two calls.
    ///
    /// This method is very cheap and can be used to prune stale requests.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }

    /// The item corresponding to the request.
    #[inline]
    pub fn item(&self) -> &T {
        self.item.value()
    }

    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.item.id()
    }

    /// Returns the item corresponding to this request along with its identifier.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'_, T> {
        self.item.item_ref()
    }
}

impl<T: Send + Sync + 'static> ActiveRequest<T> {
    /// Publish the preview pane.
    ///
    /// Mutable access to the pane internal to this request is not provided. Instead, the
    /// previewer must prepare the preview in a separate preview pane, and then call this
    /// method.
    ///
    /// If this method returns `Ok`, it means publication was successful and the pane has been
    /// swapped for a new empty pane. The empty pane should be reused to prepare the next
    /// request. If this method returns `Err`, it means that the request was cancelled, in which
    /// case the pane is unmodified.
    pub fn publish(self, pane: &mut Pane) -> Result<(), CancelledRequest<T>> {
        let Self { writer, item } = self;
        if writer.publish(pane) {
            Ok(())
        } else {
            Err(CancelledRequest { item })
        }
    }

    /// Check if the request has been cancelled by the picker.
    ///
    /// If this method returns `true`, the picker has cancelled the request and the preview
    /// pane is no longer required by the picker. In particular, the next call to [`publish`](Self::publish)
    /// is guaranteed to fail. If this method returns false, the next
    /// call to [`publish`](Self::publish) could still fail if the request is cancelled in between
    /// these two method calls.
    ///
    /// This method is very cheap. When preview generation is expensive, implementors should
    /// periodically call this method to avoid unnecessary work if possible.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }

    /// The item corresponding to the request.
    #[inline]
    pub fn item(&self) -> &T {
        self.item.value()
    }

    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.item.id()
    }

    /// Returns the item corresponding to this request along with its identifier.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'_, T> {
        self.item.item_ref()
    }
}

impl<T: Send + Sync + 'static> CancelledRequest<T> {
    /// Returns the item corresponding to this request.
    #[inline]
    pub fn item(&self) -> &T {
        self.item.value()
    }

    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.item.id()
    }

    /// Returns the item corresponding to this request along with its identifier.
    #[inline]
    pub fn item_ref(&self) -> ItemRef<'_, T> {
        self.item.item_ref()
    }
}

impl<T> fmt::Debug for CancelledRequest<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelledRequest")
            .field("id", &self.item.id())
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Display for CancelledRequest<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("preview request was cancelled")
    }
}

impl<T> std::error::Error for CancelledRequest<T> {}

#[cfg(test)]
pub(super) mod tests;
