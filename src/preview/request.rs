//! # Previewer request types
//!
//! This module contains the core types used to coordinate request state between a picker and a
//! (asynchronous) previewer. This is mainly relevant if you are designing a custom
//! [`Preview`](super::Preview) implementation.
//!
//! ## Overview
//!
//! 1. The basic type is the [`PreviewRequest`]. This request is received when the preview method is
//!    called. There are two options: handle the request immediately with
//!    [`PreviewRequest::ready`], or handle the request later with [`PreviewRequest::defer`].
//! 2. If you defer a request, you get a [`QueuedPreviewRequest`]. This represents a request which
//!    you will work on later. When you are ready to process the request, call [`QueuedPreviewRequest::start`].
//! 3. After starting work on a queued request, you get an [`ActivePreviewRequest`]. Publish the
//!    preview buffer when finished with [`ActivePreviewRequest::publish`].
//!
//! ## Request coordination with the picker
//!
//! The [`PreviewRequest`] is sent by the picker when it calls the [`preview`](super::Preview::preview) method.
//! This type represents a synchronous request for a preview. If the preview can be immediately
//! satisfied, call [`PreviewRequest::ready`], fill the preview buffer, and return the preview buffer in
//! the response.
//!
//! If the preview cannot immediately be generated, call [`PreviewRequest::defer`]. This returns a
//! [`QueuedPreviewRequest`] as well as a handle, to be returned to the picker, which is associated with
//! the queued request. The queued request should be sent to another thread to be completed
//! asynchronously so that the picker can continue rendering the frame.
//!
//! A queued request may be cancelled at any time, and the underlying request can be re-submitted.
//! The picker will do this in order to re-prioritize requests when there is a backlog of successful
//! previews.
//!
//! When the queued request is ready to be handled, convert to an [`ActivePreviewRequest`] with the
//! [`QueuedPreviewRequest::start`] method. This will prevent cancellation for re-prioritization:
//! once this method is called, the previewer will wait for this request to be satisfied without
//! requesting it again (unless the request is dropped). Note that the preview request may still be cancelled
//! if the preview is no longer required. When the preview itself is ready, call [`ActivePreviewRequest::publish`]
//! to make the preview available to the picker.
//!
//! When a queued request is very outdated, the picker will cancel the request regardless of its
//! state. When the picker restarts or exits, all open requests will be cancelled before the previewer drops.
//!
//! If the previewer wishes to cancel a given request, it may simply drop the request. The picker
//! will resubmit the request on a subsequent update. Note that repeated drops will cause the
//! preview to display indefinitely as loading: if the request was dropped because it cannot be
//! satisfied, the previewer might consider instead completing the request by publishing an error.
//!
//! ### Basic example
//!
//! Here is a simple coordination example with a single preview worker at the other end of a MPSC
//! channel.
//!
//! ```
//! use std::sync::mpsc::{Receiver, SendError, Sender};
//! use nucleo_picker::preview::{
//!     PreviewBuffer,
//!     request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
//! };
//!
//! // Queue a request for completion.
//! fn enqueue(
//!     request: PreviewRequest<'_, String>,
//!     worker: &Sender<QueuedPreviewRequest<String>>,
//! ) -> Result<PreviewResponse, SendError<QueuedPreviewRequest<String>>> {
//!     let (pending, queued) = request.defer();
//!     worker.send(queued)?;
//!     Ok(PreviewResponse::Pending(pending))
//! }
//!
//! // Complete the queued preview requests (in a separate thread).
//! fn complete(receiver: Receiver<QueuedPreviewRequest<String>>) {
//!     // reusable scratch space for all requests
//!     let mut buffer = PreviewBuffer::new();
//!
//!     while let Ok(queued) = receiver.recv() {
//!         let Ok(request) = queued.start() else {
//!             continue;
//!         };
//!         buffer.push_text(request.item());
//!         if request.publish(&mut buffer).is_err() {
//!             buffer.clear();
//!         }
//!     }
//! }
//! ```
//!
//! ## Buffer reuse
//!
//! The [`PreviewRequest`] internally contains a pre-cleared buffer that may be recycled from a
//! preview preview pane. When publishing a buffer through [`ActivePreviewRequest::publish`], the
//! worker should maintain a separate scratch buffer and swap it with the request buffer on
//! publication. If publication succeeds, the buffer will be empty, but if it fails the buffer will
//! have the original contents which the worker must clear manually.
//!
//! ## Previewing item and lifetimes
//!
//! The item for which the preview as originally requested is always available using an associated
//! `item` method; see [`PreviewRequest::item`], [`QueuedPreviewRequest::item`], and
//! [`ActivePreviewRequest::item`]. Internally, the requests uses atomic reference counting to
//! hold on to the underlying item pool (shared with the picker) as long as any request is
//! not dropped. This means that the items will remain in memory even if the picker shuts down,
//! if there are any remaining requests which have not been dropped.

use nucleo::{DetachedItem, Snapshot};
use std::fmt;

use super::{
    PreviewBuffer,
    lock::{ActiveWriter, QueuedWriter, Reader, request},
};

/// An opaque item identifier which uniquely identifies an item `T` within its originating picker.
///
/// It is guaranteed that if two [`ItemId`]s obtained *from the same picker* are equal, then the
/// corresponding items are also identical (i.e., they represent the same memory location of the underlying
/// item `&T`). In particular, this contract is upheld across restarts.
///
/// However, *different pickers may reuse identifiers*. Therefore, identifiers originating from
/// different pickers *may be the same, even if the underlying item is different*. Use caution! If
/// you require a more robust identification scheme, you must store this identity inside the item
/// itself.
///
/// Note that ordering has no relationship to ranking; it is only provided for convenience of use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub(crate) generation: u32,
    pub(crate) index: u32,
}

/// The outcome of a preview request.
pub enum PreviewResponse {
    /// The preview could be generated immediately.
    Ready(PreviewBuffer),
    /// Preview generation has been deferred.
    Pending(PendingPreview),
}

/// A request to generate a preview.
///
/// This type has a lifetime `'a` attached to the picker, but once the request is [deferred](Self::defer)
/// the lifetime is no longer held.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct PreviewRequest<'a, T> {
    pub(super) buffer: PreviewBuffer,
    pub(super) epoch: u64,
    pub(super) snapshot: &'a Snapshot<T>,
    pub(super) id: ItemId,
}

/// A preview request waiting to be processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct QueuedPreviewRequest<T> {
    writer: QueuedWriter<PreviewBuffer>,
    item: DetachedItem<T>,
    id: ItemId,
}

/// A preview request that is currently being processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct ActivePreviewRequest<T> {
    writer: ActiveWriter<PreviewBuffer>,
    item: DetachedItem<T>,
    id: ItemId,
}

/// A preview request that was cancelled by the picker.
///
/// This retains the original item and keeps the item pool alive until dropped. See the
/// [module-level documentation](crate::preview::request) for more detail.
pub struct CancelledPreviewRequest<T> {
    item: DetachedItem<T>,
    id: ItemId,
}

/// A subscription to a pending preview.
pub struct PendingPreview {
    pub(super) reader: Reader<PreviewBuffer>,
    pub(super) epoch: u64,
}

impl<T> PreviewRequest<'_, T> {
    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// Declare that the preview is complete and obtain an internal buffer to return to the picker.
    ///
    /// # Reducing allocations
    ///
    /// It is valid for the caller to allocate a new [`PreviewBuffer`] and return it
    /// directly, ignoring the preview request. However, it is encouraged to re-use the
    /// [`PreviewBuffer`] obtained using this method since the buffer may be recycled from previous
    /// requests, decreasing allocation overhead.
    pub fn ready(self) -> PreviewBuffer {
        self.buffer
    }
}

impl<'a, T: Send + Sync + 'static> PreviewRequest<'a, T> {
    /// The item corresponding to the request.
    pub fn item(&self) -> &'a T {
        // SAFETY: the index is guaranteed to be valid for this snapshot
        unsafe { self.snapshot.get_item_unchecked(self.id.index).data }
    }

    /// Defer handling of the preview request.
    ///
    /// The resulting [`PendingPreview`] will be notified when the preview request has been handled,
    /// and it must be returned to the picker. The corresponding [`QueuedPreviewRequest`] is used to
    /// complete the preview request when ready.
    pub fn defer(self) -> (PendingPreview, QueuedPreviewRequest<T>) {
        let Self {
            buffer,
            epoch,
            snapshot,
            id,
        } = self;
        // SAFETY: the request index is initialized in this snapshot, whose borrow prevents restart.
        let item = unsafe { snapshot.get_detached_item_unchecked(id.index) };
        let (reader, writer) = request(buffer);
        (
            PendingPreview { reader, epoch },
            QueuedPreviewRequest { writer, item, id },
        )
    }
}

impl<T> QueuedPreviewRequest<T> {
    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The item corresponding to the request.
    pub fn item(&self) -> &T {
        self.item.item().data
    }

    /// Declare that the previewer is ready to start generating the preview for this queued request.
    ///
    /// This method should be called when the request is actively being processed. Calling this
    /// method will prevent the picker from cancelling and resubmitting the preview request in
    /// order to increase priority.
    ///
    /// This method fails if the picker cancelled the request before it could start.
    pub fn start(self) -> Result<ActivePreviewRequest<T>, CancelledPreviewRequest<T>> {
        let Self { writer, item, id } = self;
        match writer.start() {
            Some(writer) => Ok(ActivePreviewRequest { writer, item, id }),
            None => Err(CancelledPreviewRequest { item, id }),
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
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }
}

impl<T> ActivePreviewRequest<T> {
    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.id
    }

    /// The item corresponding to the request.
    pub fn item(&self) -> &T {
        self.item.item().data
    }

    /// Publish the preview.
    ///
    /// Mutable access to the buffer internal to this request is not provided. Instead, the
    /// previewer must prepare the preview in a separate preview buffer, and then call this
    /// method.
    ///
    /// If this method returns `Ok`, it means publication was successful and the buffer has been
    /// swapped for a new empty buffer. The empty buffer should be reused to prepare the next
    /// request. If this method returns `Err`, it means that the request was cancelled, in which
    /// case the buffer is unmodified.
    pub fn publish(self, buffer: &mut PreviewBuffer) -> Result<(), CancelledPreviewRequest<T>> {
        let Self { writer, item, id } = self;
        if writer.publish(buffer) {
            Ok(())
        } else {
            Err(CancelledPreviewRequest { item, id })
        }
    }

    /// Check if the request has been cancelled by the picker.
    ///
    /// If this method returns `true`, this means that the picker has cancelled the reque
    /// the preview pane is no longer required by the picker. This means that the next ca
    /// [`publish`](Self::publish) is guaranteed to fail. If this method returns false, the next
    /// call to [`publish`](Self::publish) could still fail if the request is cancelled in between
    /// these two method calls.
    ///
    /// This method is very cheap. When preview generation is expensive, implementors should
    /// periodically call this method to avoid unnecessary work if possible.
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }
}

impl<T> CancelledPreviewRequest<T> {
    /// Returns the item corresponding to this request.
    pub fn item(&self) -> &T {
        self.item.item().data
    }

    /// Returns a unique identifier for the item in this request.
    pub fn id(&self) -> ItemId {
        self.id
    }
}

impl<T> fmt::Debug for CancelledPreviewRequest<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelledPreviewRequest")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Display for CancelledPreviewRequest<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("preview request was cancelled")
    }
}

impl<T> std::error::Error for CancelledPreviewRequest<T> {}

#[cfg(test)]
mod tests;
