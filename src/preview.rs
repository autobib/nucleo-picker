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
//! proceed with the [`request`] module and types. There are also full implementation examples in the
//! [examples folder](https://github.com/autobib/nucleo-picker/tree/master/examples)
//! on GitHub.

mod buffer;
mod cache;
mod draw;
mod lock;
pub(crate) mod pane;
mod picker;
mod pool;
pub mod request;
mod scroll;

use std::{convert::Infallible, num::NonZero, time::Duration};

pub use buffer::{PreviewBuffer, PreviewLine};
pub use picker::PreviewPicker;
pub use pool::{PoolPreviewer, PreviewWorker};
use request::{PreviewRequest, PreviewResponse};
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
///         item: &Item,
///         request: PreviewRequest<'_, Item>,
///         _timeout: Duration,
///     ) -> Result<PreviewResponse, Infallible> {
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
///
/// Note that the buffer is already cleared when it is passed to the closure.
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
/// let mut picker = Picker::new(render);
/// picker.push_batch([
///     Item { name: "red", description: "RGB: 255, 0, 0" },
///     Item { name: "blue", description: "RGB: 0, 0, 255" },
/// ]);
/// let previewer = SyncPreviewer(|item: &Item, buffer: &mut PreviewBuffer| {
///     buffer.push_text(item.description);
/// });
/// if let Some(item) = picker.with_preview(previewer).pick()? {
///     println!("{}", item.name);
/// }
/// # Ok(())
/// # }
/// ```
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
