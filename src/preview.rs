//! # Render item previews in the picker
//!
//! This module contains the core [`Preview`] trait and associated types. A *previewer* is a type
//! which knows how to generate previews for the item `T` in the picker. A preview is a special
//! window rendered beside the active matches and shows some extra information about the match which
//! is currently highlighted.
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
//! a fast previewer. For example:
//! A fast preview typically looks something like
//! TODO: example
//!
//! ### Implementing slow (asynchronous) previews

mod buffer;
mod lock;
mod picker;

use std::time::Duration;

pub use buffer::{PreviewBuffer, PreviewLine};
use lock::{ActiveWriter, QueuedWriter, Reader, request};
pub use picker::PreviewPicker;

/// Types which know how to generate previews of items.
///
/// There are a number of caveats to implementing this trait which are not required for correctness
/// but are essential for good performance of the picker interface.
///
/// # Do not block the picker
///
/// The picker will block while waiting for the [`preview`](Self::preview) method to return. The
/// provided timeout is a hint to the previewer which indicates how much time remains before the
/// next frame should be rendered. If the preview method does not return in time, the picker
/// interface will lag. In practice, the timeout will always be at least 2ms, so the duration does
/// not need to be checked if rendering the preview would less time.
///
/// When the preview can be generated in time, this should be done to reduce overhead.
/// For example, this is typically the case if the contents of the preview depend only on data
/// stored inside the item `T`. However, if preview generation could take longer than the timeout,
/// then preview generation should be deferred using [`PreviewRequest::defer`]. For example, the previewer
/// may choose to process the queued request in a separate thread.
///
/// # Preview priority
///
/// The picker will only request previews for items which are either immediately required by the
/// interface. In particular, requests should be handled last-in first-out (LIFO) order. Since
/// preview requests may become stale (for instance, if the highlighted item has moved many
/// times and there is backlog), the picker will re-prioritize preview requests by cancelling
/// old requests and calling this method again.
///
/// # Handling errors
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
        request: PreviewRequest,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr>;
}

impl<T, P: Preview<T>> Preview<T> for &mut P {
    type AbortErr = P::AbortErr;

    fn preview(
        &mut self,
        item: &T,
        request: PreviewRequest,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        (*self).preview(item, request, timeout)
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
/// # Non-blocking
///
/// All of the operations associated with a [`QueuedPreviewRequest`] and [`ActivePreviewRequest`] are
/// lock-free and wait-free. To enforce this contract, direct access to the preview buffer is not
/// provided: instead, the previewer must prepare the preview separately, and publish it using
/// [`ActivePreviewRequest::publish`]. On successful publication, a new buffer will be provided in order to
/// minimize allocation.
pub struct PreviewRequest {
    buffer: PreviewBuffer,
    epoch: u64,
}

/// A preview request waiting to be processed.
///
/// See the [`PreviewRequest`] documentation for more detail.
pub struct QueuedPreviewRequest {
    writer: QueuedWriter<PreviewBuffer>,
}

/// A preview request that is currently being processed.
///
/// See the [`PreviewRequest`] documentation for more detail.
pub struct ActivePreviewRequest {
    writer: ActiveWriter<PreviewBuffer>,
}

/// A subscription to a pending preview.
#[expect(unused)]
pub struct PendingPreview {
    reader: Reader<PreviewBuffer>,
    epoch: u64,
}

/// A single entry in the preview cache.
#[expect(unused)]
pub(crate) struct Cached {
    pub scroll_position: usize,
    pub state: Option<State>,
}

/// The status of a preview request.
#[expect(unused)]
pub(crate) enum State {
    Pending(PendingPreview),
    Ready(PreviewBuffer),
}

/// A preview buffer that is not ready yet.
#[expect(unused)]
pub(crate) enum BufferNotReady {
    /// The preview request is waiting to be processed.
    Queued(State),
    /// The preview request is currently being processed.
    Active(State),
    /// The request was dropped.
    Dropped(PreviewBuffer),
}

impl State {
    /// Obtain the preview buffer if it is ready, without blocking.
    ///
    /// If the preview is queued or active, the corresponding state is returned in the `Err` variant.
    #[expect(unused)]
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

impl PreviewRequest {
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

    /// Defer handling of the preview request.
    ///
    /// The resulting [`PendingPreview`] will be notified when the preview request has been handled,
    /// and it must be returned to the picker. The corresponding [`QueuedPreviewRequest`] is used to
    /// complete the preview request when ready.
    pub fn defer(self) -> (PendingPreview, QueuedPreviewRequest) {
        let Self { buffer, epoch } = self;
        let (reader, writer) = request(buffer);
        (
            PendingPreview { reader, epoch },
            QueuedPreviewRequest { writer },
        )
    }
}

impl QueuedPreviewRequest {
    /// Declare that the previewer is ready to start generating the preview for this queued request.
    ///
    /// This method should be called when the request is actively being processed. Calling this
    /// method will prevent the picker from cancelling and resubmitting the preview request in
    /// order to increase priority.
    ///
    /// The picker may still cancel the request if the preview pane is no longer required, in which
    /// case this method will return [`None`].
    pub fn start(self) -> Option<ActivePreviewRequest> {
        self.writer
            .start()
            .map(|writer| ActivePreviewRequest { writer })
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

impl ActivePreviewRequest {
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
    #[must_use = "publication will fail if the request was cancelled"]
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
