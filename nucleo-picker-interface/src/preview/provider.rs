use std::time::Duration;

use nucleo_picker_core::{
    preview::{Buffer, Pane},
    session::ItemRef,
};

use super::request::{Request, Response};

/// Types which know how to generate previews of items.
///
/// In most simple cases, you don't need to implement this trait yourself: see [`SyncPreviewer`] and
/// [`PoolPreviewer`](super::PoolPreviewer). For more complex use-cases, read on!
///
/// # Implementation caveats
///
/// There are a number of caveats to implementing this trait which are not required for correctness
/// but are essential for good performance of the picker interface.
///
/// ## Do not block the picker
///
/// The picker will block while waiting for any of the associated methods to return.
/// For [`preview`](Self::preview) this can be quite sensitive, and the
/// provided timeout is a hint to the previewer which indicates how much time remains before the
/// next frame should be rendered. If the preview method does not return in time, the picker
/// interface may lag.
///
/// When the preview can be generated in time, this should be done to reduce overhead.
/// For example, this is typically the case if the contents of the preview depend only on data
/// stored inside the item `T`. However, if preview generation could take longer than the timeout,
/// then preview generation should be deferred using [`Request::defer`]. For example, the previewer
/// may choose to process the queued request in a separate thread.
///
/// ## Preview priority
///
/// The picker will only request previews for items which are immediately required by the
/// interface. In particular, requests should be handled in last-in first-out (LIFO) order. Since
/// preview requests may become stale (for instance, if the hovered item has moved many
/// times and there is backlog), the picker will re-prioritize preview requests by cancelling
/// old requests and calling this method again.
///
/// ## Prefetch
///
/// A previewer can also optionally enable prefetch support by setting
/// [`prefetch_limit`](Self::prefetch_limit) to a non-zero value and overriding the default
/// behaviour of [`prefetch`](Self::prefetch).
///
/// Prefetch is used to make preview requests for items which the user may scroll to in the near
/// future. This can greatly improve the perceived responsiveness of the preview pane, especially in
/// the presence of slow previews. However, implementations should be careful not to treat prefetch
/// responses with higher priority than normal responses, which are sent in response to the actual
/// preview pane that the user currently requires.
///
/// See the [`prefetch`](Self::prefetch) documentation for more details.
///
/// ## Handling errors
///
/// ### Non-fatal errors
///
/// For non-fatal errors, for instance errors which occur while generating a preview for a
/// specific item, the previewer should instead write an appropriate error message directly into the preview buffer.
///
/// Note that the item may be resubmitted again in the future. This will happen if the preview drops out of the
/// preview cache, or at manual request via [`PreviewEvent::Refresh`](nucleo_picker_core::event::PreviewEvent::Refresh)
/// (available by default to the user with `ctrl + r`).
///
/// ```
/// use std::time::Duration;
/// use nucleo_picker_interface::preview::{Preview, request::{Request, Response}};
///
/// type Item = Result<String, String>;
/// struct ResultPreview;
///
/// impl Preview<Item> for ResultPreview {
///     fn preview(
///         &mut self,
///         request: Request<'_, Item>,
///         _timeout: Duration,
///     ) -> Response {
///         let item = request.item();
///         let mut pane = request.ready();
///         match item {
///             Ok(text) => pane.push_text(text),
///             Err(message) => {
///                 pane.set_err(true);
///                 pane.push_text(message);
///             }
///         }
///         Response::Ready(pane)
///     }
/// }
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub trait Preview<T> {
    /// Generate a preview.
    ///
    /// The item to which the preview corresponds is available with [`Request::item`]. The
    /// [`Duration`] is a timeout suggested by the picker and depends on the current frame budget. Exceeding the budget may cause the picker to lag.
    ///
    /// See the [trait-level docs](Preview) for more detail.
    fn preview(&mut self, request: Request<'_, T>, timeout: Duration) -> Response;

    /// Configure the preview range for prefetch.
    ///
    /// Prefetching requests previews for items which are not yet hovered, but which the user
    /// may hover soon. This can greatly reduce perceived latency at the cost of unnecessary
    /// preview generation.
    ///
    /// If this value is set to `0`, prefetch requests will never be made. The default value is `0`.
    /// When prefetch is desirable, in most cases a small integer around 4 is sufficient. For very
    /// large threadpools, larger prefetch limits can be feasible.
    fn prefetch_limit(&self) -> usize {
        0
    }

    /// Handle a speculative preview request.
    ///
    /// Return `Ok` if the request can be handled, or return the unused buffer in the `Err` variant
    /// to decline this particular request. Any request can be safely declined; if the preview is
    /// required for the hovered item, the request will be re-submitted to the usual
    /// [`preview`](Self::preview) method.
    ///
    /// Note that callers must override [`prefetch_limit`](Self::prefetch_limit) to a non-zero
    /// value, or this method will never be called.
    ///
    /// Similarly to [`preview`](Self::preview), this method must not block for too long or the
    /// picker will lag. The [`Duration`] is a timeout suggested by the picker and depends on the
    /// frame budget.
    ///
    /// ## Prefetch call sequence
    ///
    /// Call a *cycle* a single pass over the [`prefetch_limit`](Self::prefetch_limit)-nearest items
    /// in the match list. The cycle refreshes each time the hovered item changes or the match list updates.
    /// For each item, once per cycle, its status is checked:
    ///
    /// 1. If the request was dropped or no previous request was made, it will be requested.
    /// 2. If the request is queued, prefetch requests will pause until the item is processed.
    /// 3. If the request is active or published, the item is skipped.
    ///
    /// Each request is made *at most once per frame*, and moreover *only if the frame has
    /// additional time after rendering*.
    #[allow(
        clippy::result_large_err,
        reason = "declined prefetches return their reusable buffer without allocating"
    )]
    fn prefetch(
        &mut self,
        request: Request<'_, T>,
        _timeout: Duration,
    ) -> Result<Response, Buffer> {
        Err(request.buffer)
    }

    /// Handle changes to the hovered item.
    ///
    /// Hover changes are reported *before* promoting queued work or cancelling stale requests
    /// (including prefetch requests).
    ///
    /// This method is called whenever the preview pane changes to a new target. Unlike `preview`, this
    /// method is called even if the item is cached. The `None` target means that the preview is
    /// hidden or that there are no matching items (so none is hovered). This method is called
    /// before the `preview` method (if the `preview` method is called at all).
    ///
    /// The initial hover state is `None`. When the picker quits, all pending preview requests are
    /// cancelled followed by a final `hover_changed(None)` if there was an active preview pane at
    /// the end. Note that `hover_changed` is not called when panicking.
    ///
    /// This method runs on the picker thread and therefore must not block. If it blocks, the picker
    /// interface will lag.
    ///
    /// When wrapping another [`Preview`] implementation, in most cases you should forward this call
    /// to the inner previewer, since it may depend on this information for scheduling. For
    /// instance, this is the case for a [`PoolPreviewer`](super::PoolPreviewer).
    ///
    /// The default implementation does nothing.
    fn hover_changed(&mut self, _item: Option<ItemRef<'_, T>>) {}

    /// Handle an interactive picker restart.
    ///
    /// This method is called when an [interactive restart](crate::Interface::on_restart) is handled. Note that
    /// this method is called *after* clearing the preview cache, cancelling pending requests, and
    /// the call to `hover_changed(None)` (if one was made since there was a hovered item before the
    /// restart), and *before* restarting the underlying matching engine.
    ///
    /// This method runs on the picker thread and therefore must not block. If it blocks, the picker
    /// interface will lag.
    ///
    /// The default implementation does nothing.
    fn restart(&mut self) {}
}

impl<T, P: Preview<T> + ?Sized> Preview<T> for &mut P {
    #[inline]
    fn hover_changed(&mut self, item: Option<ItemRef<'_, T>>) {
        (**self).hover_changed(item);
    }

    #[inline]
    fn restart(&mut self) {
        (**self).restart();
    }

    #[inline]
    fn preview(&mut self, request: Request<'_, T>, timeout: Duration) -> Response {
        (*self).preview(request, timeout)
    }

    #[inline]
    fn prefetch_limit(&self) -> usize {
        (**self).prefetch_limit()
    }

    #[inline]
    fn prefetch(&mut self, request: Request<'_, T>, timeout: Duration) -> Result<Response, Buffer> {
        (**self).prefetch(request, timeout)
    }
}

/// An infallible synchronous previewer.
///
/// This struct wraps a `FnMut(&T, &mut Pane)` closure which fills a cleared preview pane. It is expected that the
/// function `F` will return *without delaying the frame* and *without any errors*. This can be
/// convenient for previewing types `T` which already contain all of the data required
/// for the preview.
///
/// Note that the pane is already cleared when it is passed to the closure.
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub struct SyncPreviewer<F>(
    /// The closure which populates the preview pane.
    pub F,
);

impl<T, F> Preview<T> for SyncPreviewer<F>
where
    T: Send + Sync + 'static,
    F: FnMut(&T, &mut Pane),
{
    #[inline]
    fn preview(&mut self, request: Request<'_, T>, _timeout: Duration) -> Response {
        let item = request.item();
        let mut pane = request.ready();
        (self.0)(item, &mut pane);
        Response::Ready(pane)
    }
}
