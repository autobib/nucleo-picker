//! # Render item previews in the picker
//!
//! This module contains the core [`Preview`] trait and associated types. A *previewer* is a type
//! which knows how to generate previews for the item `T` in the picker. The preview is rendered
//! in the *preview pane*, which is a special window rendered beside the active matches.
//!
//! In many cases, you can avoid the full details of implementing a previewer:
//!
//! - For fast non-blocking previews, see [`SyncPreviewer`].
//! - For slow previews that should be performed in a background threadpool, see [`PoolPreviewer`].
//!
//! If you are doing something more complicated, read on!
//!
//! ## Implementing [`Preview`]
//!
//! The [`Preview`] trait is designed around two core use-cases:
//! [fast](#implementing-fast-synchronous-previews) (synchronous) previews and
//! [slow](#implementing-slow-asynchronous-previews) (asynchronous) previews.
//!
//! ### Implementing fast (synchronous) previews
//!
//! If the preview can be generated directly from data stored in the item `T`, you most likely have
//! a fast previewer.
//! A fast preview typically looks something like
//! TODO: write
//!
//! ### Implementing slow (asynchronous) previews
//! TODO: write

mod buffer;
mod cache;
mod draw;
mod lock;
pub(crate) mod pane;
mod picker;
mod pool;
mod scroll;

use std::{convert::Infallible, num::NonZero, time::Duration};

pub use buffer::{PreviewBuffer, PreviewLine};
use lock::{ActiveWriter, QueuedWriter, Reader, request};
use nucleo::{DetachedItem, Snapshot};
pub use picker::PreviewPicker;
pub use pool::{PoolPreviewer, PreviewWorker};
pub use scroll::PreviewEvent;

/// Preview boundary characters.
///
/// This is the value used to set
/// [`PickerOptions::preview_boundary_chars`](crate::PickerOptions::preview_boundary_chars). Note
/// that all characters must have Unicode width 1 or the terminal may be corrupted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundaryChars {
    /// Top left, default `╭`.
    pub top_left: char,
    /// Top right, default `╮`
    pub top_right: char,
    /// Bottom left, default `╰`
    pub bottom_left: char,
    /// Bottom right, default `╯`
    pub bottom_right: char,
    /// Left and right edges, default `│`
    pub vertical: char,
    /// Top and bottom edges, default `─`
    pub horizontal: char,
}

impl BoundaryChars {
    /// Initialize with default Unicode boundary characters.
    ///
    /// This is the same as the [`Default`] implementation but as a `const fn`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            top_left: '╭',
            top_right: '╮',
            bottom_left: '╰',
            bottom_right: '╯',
            vertical: '│',
            horizontal: '─',
        }
    }

    /// Initialize with ASCII boundary characters.
    ///
    /// This sets `+` for corners, `|` for vertical edges, and `-` for horizontal edges.
    #[must_use]
    pub const fn ascii() -> Self {
        Self {
            top_left: '+',
            top_right: '+',
            bottom_left: '+',
            bottom_right: '+',
            vertical: '|',
            horizontal: '-',
        }
    }
}

impl Default for BoundaryChars {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal preview configuration.
#[derive(Debug, Clone)]
pub(crate) struct PreviewConfig {
    pub cache_size: Option<NonZero<usize>>,
    pub boundary_chars: BoundaryChars,
    pub line_numbers: bool,
}

impl PreviewConfig {
    pub const fn new() -> Self {
        Self {
            cache_size: NonZero::new(128),
            boundary_chars: BoundaryChars::new(),
            line_numbers: false,
        }
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Types which know how to generate previews of items.
///
/// # Implementation caveats
///
/// There are a number of caveats to implementing this trait which are not required for correctness
/// but are essential for good performance of the picker interface.
///
/// ## Do not block the picker
///
/// The picker will block while waiting for the [`preview`](Self::preview) method to return. The
/// provided timeout is a hint to the previewer which indicates how much time remains before the
/// next frame should be rendered. If the preview method does not return in time, the picker
/// interface will lag. In practice, the timeout will always be at least 2ms, so the duration does
/// not need to be checked if rendering the preview would take less time.
///
/// When the preview can be generated in time, this should be done to reduce overhead.
/// For example, this is typically the case if the contents of the preview depend only on data
/// stored inside the item `T`. However, if preview generation could take longer than the timeout,
/// then preview generation should be deferred using [`PreviewRequest::defer`]. For example, the previewer
/// may choose to process the queued request in a separate thread.
///
/// ## Preview priority
///
/// The picker will only request previews for items which are either immediately required by the
/// interface. In particular, requests should be handled in last-in first-out (LIFO) order. Since
/// preview requests may become stale (for instance, if the highlighted item has moved many
/// times and there is backlog), the picker will re-prioritize preview requests by cancelling
/// old requests and calling this method again.
///
/// ## Handling errors
///
/// If the [`preview`](Self::preview) method returns an error, the picker will immediately terminate and propagate the
/// resulting error to the caller. For non-fatal errors, for instance errors which occur while
/// generating a preview for a specific item, the previewer should instead write an appropriate
/// error message directly into the preview buffer.
pub trait Preview<T> {
    /// An unrecoverable error which may occur while generating a preview.
    type AbortErr;

    /// Generate a preview of the provided item.
    fn preview(
        &mut self,
        item: &T,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr>;
}

impl<T, P: Preview<T>> Preview<T> for &mut P {
    type AbortErr = P::AbortErr;

    fn preview(
        &mut self,
        item: &T,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        (*self).preview(item, request, timeout)
    }
}

/// An infallible synchronous previewer.
///
/// This struct wraps a `FnMut(&T, &mut PreviewBuffer)` closure which fills a cleared preview buffer. It is expected that the
/// function `F` will return *without delaying the frame* and *without any errors*. This can be
/// convenient for previewing types `T` which already contain all of the data required
/// for the preview.
pub struct SyncPreviewer<F>(
    /// The closure which populates the preview buffer.
    pub F,
);

impl<T, F> Preview<T> for SyncPreviewer<F>
where
    F: FnMut(&T, &mut PreviewBuffer),
{
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        item: &T,
        request: PreviewRequest<'_, T>,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        let mut buffer = request.ready();
        (self.0)(item, &mut buffer);
        Ok(PreviewResponse::Ready(buffer))
    }
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
/// # Request types
///
/// The [`PreviewRequest`] is sent by the picker when it calls the [`preview`](Preview::preview) method.
/// This type represents a synchronous request for a preview. If the preview can be immediately
/// satisfied, call [`PreviewRequest::ready`], fill the preview buffer, and return the preview buffer in
/// the response.
///
/// If the preview cannot immediately be generated, call [`PreviewRequest::defer`]. This returns a
/// [`QueuedPreviewRequest`] as well as a handle, to be returned to the picker, which is associated with
/// the queued request. The queued request should be sent to another thread to be completed
/// asynchronously so that the picker can continue rendering the frame.
///
/// A queued request may be cancelled at any time, and the underlying request can be re-submitted.
/// The picker will do this in order to re-prioritize requests when there is a backlog of successful
/// previews.
///
/// When the queued request is ready to be handled, convert to an [`ActivePreviewRequest`] with the
/// [`QueuedPreviewRequest::start`] method. This will prevent cancellation for re-prioritization:
/// once this method is called, the previewer will wait for this request to be satisfied without
/// requesting it again. Note that the preview request may still be cancelled if the preview is no
/// longer required. When the preview itself is ready, call [`ActivePreviewRequest::publish`] to make the
/// preview available to the picker.
///
/// When a queued request is very outdated, the picker will cancel the request regardless of its
/// state. When the picker restarts or exits, all open requests will be cancelled before the previewer drops.
///
/// # Previewing item and lifetimes
///
/// The item for which the preview as originally requested is always available using the associated
/// [`item`](Self::item) method; see [`QueuedPreviewRequest::item`] and
/// [`ActivePreviewRequest::item`]. This type has a lifetime `'a` attached to the picker, but once
/// the request is [deferred](Self::defer) the lifetime is no longer held. Internally, the request
/// uses atomic reference counting to hold on to the underlying item pool (shared with the picker)
/// as long as any request is not dropped. This means that the items will remain in
/// memory even if the picker shuts down, if there are any remaining requests which have not
/// been dropped.
pub struct PreviewRequest<'a, T> {
    buffer: PreviewBuffer,
    epoch: u64,
    snapshot: &'a Snapshot<T>,
    // the index for the current item, guaranteed to be valid in
    // the provided snapshot
    idx: u32,
}

/// A preview request waiting to be processed.
///
/// See the [`PreviewRequest`] documentation for more detail.
pub struct QueuedPreviewRequest<T> {
    writer: QueuedWriter<PreviewBuffer>,
    item: DetachedItem<T>,
}

/// A preview request that is currently being processed.
///
/// See the [`PreviewRequest`] documentation for more detail.
pub struct ActivePreviewRequest<T> {
    writer: ActiveWriter<PreviewBuffer>,
    item: DetachedItem<T>,
}

/// A subscription to a pending preview.
pub struct PendingPreview {
    reader: Reader<PreviewBuffer>,
    epoch: u64,
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
    ///    cancelling stale requests and calling the [`Preview::preview`] method again with the same
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
