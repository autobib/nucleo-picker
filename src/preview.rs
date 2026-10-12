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
//! If you are doing something more complicated, start at the [`Preview`] documentation and then
//! proceed with the [`request`] module. There are also full implementation examples in the
//! [examples folder](https://github.com/autobib/nucleo-picker/tree/master/examples)
//! on GitHub.

#[cfg(feature = "preview-ansi")]
mod ansi;
mod buffer;
mod cache;
mod draw;
mod item;
mod lock;
pub(crate) mod pane;
mod picker;
mod pool;
pub mod request;
mod scroll;
mod text;

use std::{convert::Infallible, marker::PhantomData, num::NonZero, time::Duration};

#[cfg(feature = "preview-ansi")]
pub use ansi::{AnsiState, AnsiWriter};
pub use buffer::{PreviewBuffer, PreviewLine};
pub use item::{ItemId, ItemRef};
pub use picker::PreviewPicker;
pub use pool::{PoolPreviewer, PreviewWorker};
use request::{PreviewRequest, PreviewResponse};
pub use scroll::PreviewEvent;
pub use text::TextWriter;

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
    pub ratio: f64,
    pub cache_size: Option<NonZero<usize>>,
    pub boundary_chars: BoundaryChars,
}

impl PreviewConfig {
    pub const fn new() -> Self {
        Self {
            ratio: 0.5,
            cache_size: NonZero::new(128),
            boundary_chars: BoundaryChars::new(),
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
/// In most simple cases, you don't need to implement this trait yourself: see [`SyncPreviewer`] and
/// [`PoolPreviewer`]. For more complex use-cases, read on!
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
///
/// Note that the item may be resubmitted again in the future. This will happen if the preview drops out of the
/// preview cache, or at manual request via [`PreviewEvent::Refresh`]
/// (available by default to the user with `ctrl + r`).
///
/// ```
/// use std::{convert::Infallible, time::Duration};
/// use nucleo_picker::preview::{Preview, request::{PreviewRequest, PreviewResponse}};
///
/// type Item = Result<String, String>;
/// struct ResultPreview;
///
/// impl Preview<Item> for ResultPreview {
///     type AbortErr = Infallible;
///
///     fn preview(
///         &mut self,
///         request: PreviewRequest<'_, Item>,
///         _timeout: Duration,
///     ) -> Result<PreviewResponse, Infallible> {
///         let item = request.item();
///         let mut buffer = request.ready();
///         match item {
///             Ok(text) => buffer.push_text(text),
///             Err(message) => {
///                 buffer.set_err(true);
///                 buffer.push_text(message);
///             }
///         }
///         Ok(PreviewResponse::Ready(buffer))
///     }
/// }
/// ```
pub trait Preview<T> {
    /// An unrecoverable error which may occur while generating a preview.
    type AbortErr;

    /// Generate a preview.
    ///
    /// The item to which the preview corresponds is available with [`PreviewRequest::item`]. The
    /// [`Duration`] is a timeout suggested by the picker and depende on the current frame budget. It will always be at least 2ms. Exceeding the budget will cause the picker to lag.
    ///
    /// See the [trait-level docs](Preview) for more detail.
    fn preview(
        &mut self,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr>;

    /// Handle changes to an active preview target.
    ///
    /// This method is called whenever the preview pane changes to a new target. Unlike `preview`, this
    /// method is called even if the item is cached. The `None` target means that the preview is
    /// hidden or that there are no matching items (so none is selected). This method is called
    /// before the `preview` method (if the `preview` method is called at all).
    ///
    /// This method runs on the picker thread and therefore must not block. If it blocks, the picker
    /// interface will lag.
    ///
    /// The default implementation does nothing.
    fn focus_changed(&mut self, _item: Option<ItemRef<'_, T>>) {}

    /// Handle an interactive picker restart.
    ///
    /// This method is called when an interactive restart is processed by the picker. Note that this
    /// method is called *after* clearing the preview cache, cancelling pending requests, and
    /// the call to `focus_changed(None)`, and *before* restarting the underlying match item.
    ///
    /// This method runs on the picker thread and therefore must not block. If it blocks, the picker
    /// interface will lag.
    ///
    /// The default implementation does nothing.
    fn restart(&mut self) {}
}

impl<T, P: Preview<T> + ?Sized> Preview<T> for &mut P {
    type AbortErr = P::AbortErr;

    fn focus_changed(&mut self, item: Option<ItemRef<'_, T>>) {
        (**self).focus_changed(item);
    }

    fn restart(&mut self) {
        (**self).restart();
    }

    fn preview(
        &mut self,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        (*self).preview(request, timeout)
    }
}

/// An infallible synchronous previewer.
///
/// This struct wraps a `FnMut(&T, &mut PreviewBuffer)` closure which fills a cleared preview buffer. It is expected that the
/// function `F` will return *without delaying the frame* and *without any errors*. This can be
/// convenient for previewing types `T` which already contain all of the data required
/// for the preview.
///
/// Note that the buffer is already cleared when it is passed to the closure.
///
/// ## Parametrized error
///
/// There is a parametrized abort error type `A` (which defaults to [`Infallible`]). This is for
/// compatibility with arbitrary application abort errors. Note that the
/// [`SyncPreviewer::new`] constructor uses the [`Infallible`] default; use
/// [`SyncPreviewer::with_abort`] to change the error type.
///
/// ## Example
///
/// ```no_run
/// use std::borrow::Cow;
/// use nucleo_picker::{Picker, preview::{PreviewBuffer, SyncPreviewer}};
///
/// struct Item {
///     name: &'static str,
///     description: &'static str,
/// }
///
/// fn render(item: &Item) -> Cow<'_, str> {
///     Cow::Borrowed(item.name)
/// }
///
/// # fn main() -> std::io::Result<()> {
/// let picker = Picker::new(render);
/// picker.push_batch([
///     Item { name: "red", description: "RGB: 255, 0, 0" },
///     Item { name: "blue", description: "RGB: 0, 0, 255" },
/// ]);
/// let previewer = SyncPreviewer::new(|item: &Item, buffer: &mut PreviewBuffer| {
///     buffer.push_text(item.description);
/// });
/// let mut picker = picker.with_preview(previewer);
/// if let Some(item) = picker.pick()? {
///     println!("{}", item.name);
/// }
/// # Ok(())
/// # }
/// ```
pub struct SyncPreviewer<F, A = Infallible> {
    /// The closure which populates the preview buffer.
    preview: F,
    abort: PhantomData<fn() -> A>,
}

impl<F> SyncPreviewer<F> {
    /// Initialize a new previewer by wrapping a closure.
    #[must_use]
    pub const fn new(preview: F) -> Self {
        Self {
            preview,
            abort: PhantomData,
        }
    }
}

impl<F, A> SyncPreviewer<F, A> {
    /// Set the application abort type.
    ///
    /// Note that preview generation is still infallible. This is only for compatibility with
    /// arbitrary application-defined errors.
    #[must_use]
    pub fn with_abort<B>(self) -> SyncPreviewer<F, B> {
        SyncPreviewer {
            preview: self.preview,
            abort: PhantomData,
        }
    }

    /// Recover the original previewing closure.
    #[must_use]
    pub fn into_inner(self) -> F {
        self.preview
    }
}

impl<T, F, A> Preview<T> for SyncPreviewer<F, A>
where
    T: Send + Sync + 'static,
    F: FnMut(&T, &mut PreviewBuffer),
{
    type AbortErr = A;

    fn preview(
        &mut self,
        request: PreviewRequest<'_, T>,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        let item = request.item();
        let mut buffer = request.ready();
        (self.preview)(item, &mut buffer);
        Ok(PreviewResponse::Ready(buffer))
    }
}
