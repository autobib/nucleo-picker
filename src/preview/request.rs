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

use super::{
    PreviewBuffer,
    lock::{ActiveWriter, QueuedWriter, Reader, request},
};

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
    // the index for the current item, guaranteed to be valid in
    // the provided snapshot
    pub(super) idx: u32,
}

/// A preview request waiting to be processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct QueuedPreviewRequest<T> {
    writer: QueuedWriter<PreviewBuffer>,
    item: DetachedItem<T>,
}

/// A preview request that is currently being processed.
///
/// See the [module-level documentation](crate::preview::request) for more detail.
pub struct ActivePreviewRequest<T> {
    writer: ActiveWriter<PreviewBuffer>,
    item: DetachedItem<T>,
}

/// A subscription to a pending preview.
pub struct PendingPreview {
    pub(super) reader: Reader<PreviewBuffer>,
    pub(super) epoch: u64,
}

impl<T> PreviewRequest<'_, T> {
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
        unsafe { self.snapshot.get_item_unchecked(self.idx).data }
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
            idx,
        } = self;
        // SAFETY: the request index is initialized in this snapshot, whose borrow prevents restart.
        let item = unsafe { snapshot.get_detached_item_unchecked(idx) };
        let (reader, writer) = request(buffer);
        (
            PendingPreview { reader, epoch },
            QueuedPreviewRequest { writer, item },
        )
    }
}

impl<T> QueuedPreviewRequest<T> {
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
    /// The picker may still cancel the request if the preview pane is no longer required, in which
    /// case this method will return [`None`].
    pub fn start(self) -> Option<ActivePreviewRequest<T>> {
        self.writer.start().map(|writer| ActivePreviewRequest {
            writer,
            item: self.item,
        })
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
    /// If this method returns true, the next call to [`start`](Self::start) is guaranteed to
    /// return [`None`]. Note that if this method returns false, the next call to
    /// [`start`](Self::start) could still return [`None`] if the request is cancelled in between
    /// these two method calls.
    ///
    /// This method is very cheap and can be used to prune stale requests.
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }
}

impl<T> ActivePreviewRequest<T> {
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
    /// If this method returns `true`, it means publication was successful and the buffer has been
    /// swapped for a new empty buffer. The empty buffer should be reused to prepare the next
    /// request.
    ///
    /// If this method returns `false`, it means that the request was cancelled, in which case the
    /// buffer is unmodified.
    pub fn publish(self, buffer: &mut PreviewBuffer) -> bool {
        self.writer.publish(buffer)
    }

    /// Check if the request has been cancelled by the picker.
    ///
    /// If this method returns `true`, this means that the picker has cancelled the request because
    /// the preview pane is no longer required by the picker. This means that the next call to
    /// [`publish`](Self::publish) is guaranteed to return `false`. If this method returns false, the next
    /// call to [`publish`](Self::publish) could still return `false` if the request is cancelled in between
    /// these two method calls.
    ///
    /// This method is very cheap. When preview generation is expensive, implementors should
    /// periodically call this method to avoid unnecessary work if possible.
    pub fn is_cancelled(&self) -> bool {
        self.writer.is_cancelled()
    }
}
