//! # Render item previews in the picker
//!
//! This module contains the core [`Preview`] trait and associated types. A *previewer* is a type
//! which knows how to generate previews for the item `T` in the picker. The preview is rendered
//! in the *preview pane*, which is a special window rendered beside the active matches.
//!
//! In many cases, you can avoid the full details of implementing a previewer:
//!
//! - For fast non-blocking previews, see [`SyncPreviewer`].
//! - For slow previews that should be performed in a background thread pool, see [`PoolPreviewer`].
//!
//! If you are doing something more complicated, start at the [`Preview`] documentation and then
//! proceed with the [`request`] module. Here are some other useful resources:
//!
//! - There are full implementation examples in the [examples folder](https://github.com/autobib/nucleo-picker/tree/master/examples)
//!   on GitHub.
//! - Default keybindings for preview events can be found in the [picker usage documetation](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md)
//! - For documentation concerning the internal preview cache, see the [preview interface
//!   docs](WithPreview#preview-cache).
//! - There is a [basic example](#basic-example), [extended example](#extended-example), and a
//!   [simple background preview example](#background-preview-example).
//!
//! ## Using previews
//!
//! In short, start with a [`Picker`](crate::Picker) and add a previewer using
//! [`Picker::with_preview`](crate::Picker::with_preview). The type transitions are compatible with
//! [`Picker::multi`](crate::Picker::multi) for previewing pickers with multiple selections.
//!
//! For one-off usage, the above methods are typically sufficient. Note that if `P` implements
//! [`Preview`], then `&mut P` also implements [`Preview`]: this can be useful if the previewer has
//! state which should be preserved between calls or which should be accessed after selection is
//! complete.
//!
//! ## Examples
//!
//! ### Basic example
//!
//! Here is an example highlighting basic usage.
//!
//! ```no_run
//! use nucleo_picker::{
//!     Multi, Picker,
//!     preview::{PreviewBuffer, SyncPreviewer},
//!     render::StrRenderer,
//! };
//!
//! # fn main() -> std::io::Result<()> {
//! let picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//!
//! // this is a bit of a silly previewer, which just previews the item
//! // text itself, but it's enough for the example
//! let previewer = SyncPreviewer(|color: &&str, buffer: &mut PreviewBuffer| {
//!     buffer.push_text(color);
//! });
//!
//! // run the picker with previews
//! let mut picker = picker.with_preview(previewer);
//! if let Some(color) = picker.pick()? {
//!     println!("{color}");
//! }
//!
//! // run the picker without previews and with multiple selections
//! // without consuming the original picker or previewer
//! for color in picker.as_interface(Multi).pick()? {
//!     println!("{color}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Extended example
//!
//! This example uses a stateful previewer. Run this example with
//! `cargo run --release --example preview_style --features preview`.
//!
//! ```no_run
#![doc = include_str!("../examples/preview_style.rs")]
//! ```
//!
//! ### Background preview example
//!
//! Here is a basic example of a previewer running in a background thread. This also uses the
//! [`PreviewWorker`] trait, which is the same abstraction used by a [`PoolPreviewer`].
//!
//! ```no_run
#![doc = include_str!("../examples/preview_single.rs")]
//! ```

#[cfg(feature = "preview-ansi")]
mod ansi;
mod buffer;
mod cache;
mod draw;
mod lock;
pub(crate) mod pane;
mod pool;
pub mod request;
mod scroll;
mod text;

use std::{num::NonZero, time::Duration};

#[doc(inline)]
pub use crate::session::{ItemId, ItemRef, WithPreview};
#[cfg(feature = "preview-ansi")]
pub use ansi::{AnsiState, AnsiWriter};
pub use buffer::{PreviewBuffer, PreviewLine};
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
/// The picker will only request previews for items which are immediately required by the
/// interface. In particular, requests should be handled in last-in first-out (LIFO) order. Since
/// preview requests may become stale (for instance, if the highlighted item has moved many
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
/// preview cache, or at manual request via [`PreviewEvent::Refresh`]
/// (available by default to the user with `ctrl + r`).
///
/// ```
/// use std::time::Duration;
/// use nucleo_picker::preview::{Preview, request::{PreviewRequest, PreviewResponse}};
///
/// type Item = Result<String, String>;
/// struct ResultPreview;
///
/// impl Preview<Item> for ResultPreview {
///     fn preview(
///         &mut self,
///         request: PreviewRequest<'_, Item>,
///         _timeout: Duration,
///     ) -> PreviewResponse {
///         let item = request.item();
///         let mut buffer = request.ready();
///         match item {
///             Ok(text) => buffer.push_text(text),
///             Err(message) => {
///                 buffer.set_err(true);
///                 buffer.push_text(message);
///             }
///         }
///         PreviewResponse::Ready(buffer)
///     }
/// }
/// ```
///
/// ### Fatal errors
///
/// For fatal errors, it is recommended to pass a handle to the
/// [`EventSource`](crate::event::EventSource) and propagate errors directly to the picker using
/// [`Event::Abort`](crate::event::Event::Abort). For instance, if using a channel with
/// [`StdinEventSender`](crate::event::StdinEventSender), one can pass senders to the
/// previewer or preview workers directly.
///
/// See the [online preview example](https://github.com/autobib/nucleo-picker/blob/master/examples/online_preview.rs)
/// for a complete example highlighting propagation of fatal preview errors.
///
/// There are implementation examples in the [module-level docs](self).
pub trait Preview<T> {
    /// Generate a preview.
    ///
    /// The item to which the preview corresponds is available with [`PreviewRequest::item`]. The
    /// [`Duration`] is a timeout suggested by the picker and depends on the current frame budget. It will always be at least 2ms. Exceeding the budget will cause the picker to lag.
    ///
    /// See the [trait-level docs](Preview) for more detail.
    fn preview(&mut self, request: PreviewRequest<'_, T>, timeout: Duration) -> PreviewResponse;

    /// Configure the preview range for prefetch.
    ///
    /// Prefetching requests previews for items which are not yet highlighted, but which the user
    /// may highlight soon. This can greatly reduce perceived latency at the cost of unnecessary
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
    /// required for the current item, the request will be re-submitted to the usual
    /// [`preview`](Self::preview) method.
    ///
    /// Note that callers must override [`prefetch_limit`](Self::prefetch_limit) to a non-zero
    /// value, or this method will never be called.
    ///
    /// Similarly to [`preview`](Self::preview), this method must not block for too long or the
    /// picker will lag. The [`Duration`] is a timeout suggested by the picker and depends on the
    /// frame budget; it will always be at least 2ms.
    ///
    /// ## Prefetch call sequence
    ///
    /// Call a *cycle* a single pass over the [`prefetch_limit`](Self::prefetch_limit)-nearest items
    /// in the match list. The cycle refreshes each time the cursor moves or the match list updates.
    /// For each item, once per cycle, its status is checked:
    ///
    /// 1. If the request was dropped or no previous request was made, it will be requested.
    /// 2. If the request is queued, prefetch requests will pause until the item is processed.
    /// 3. If the request is active or published, the item is skipped.
    ///
    /// Each request is made *at most once per frame*, and moreover *only if the frame has
    /// additional time after rendering*.
    fn prefetch(
        &mut self,
        request: PreviewRequest<'_, T>,
        _timeout: Duration,
    ) -> Result<PreviewResponse, PreviewBuffer> {
        Err(request.buffer)
    }

    /// Handle changes to an active preview target.
    ///
    /// This method is called whenever the preview pane changes to a new target. Unlike `preview`, this
    /// method is called even if the item is cached. The `None` target means that the preview is
    /// hidden or that there are no matching items (so none is selected). This method is called
    /// before the `preview` method (if the `preview` method is called at all).
    ///
    /// The initial focus state is `None`. When the picker quits, all pending preview requests are
    /// cancelled followed by a final `focus_changed(None)` if there was an active preview pane at
    /// the end. Note that `focus_changed` is not called when panicking.
    ///
    /// This method runs on the picker thread and therefore must not block. If it blocks, the picker
    /// interface will lag.
    ///
    /// When wrapping another [`Preview`] implementation, in most cases you should forward this call
    /// to the inner previewer, since it may depend on this information for scheduling. For
    /// instance, this is the case for a [`PoolPreviewer`].
    ///
    /// The default implementation does nothing.
    fn focus_changed(&mut self, _item: Option<ItemRef<'_, T>>) {}

    /// Handle an interactive picker restart.
    ///
    /// This method is called when an interactive restart (that is,
    /// [`Event::Restart`](crate::event::Event::Restart)) is processed by the picker. Note that
    /// this method is called *after* clearing the preview cache, cancelling pending requests, and
    /// the call to `focus_changed(None)` (if one was made since the focus was active before the
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
    fn focus_changed(&mut self, item: Option<ItemRef<'_, T>>) {
        (**self).focus_changed(item);
    }

    #[inline]
    fn restart(&mut self) {
        (**self).restart();
    }

    #[inline]
    fn preview(&mut self, request: PreviewRequest<'_, T>, timeout: Duration) -> PreviewResponse {
        (*self).preview(request, timeout)
    }

    #[inline]
    fn prefetch_limit(&self) -> usize {
        (**self).prefetch_limit()
    }

    #[inline]
    fn prefetch(
        &mut self,
        request: PreviewRequest<'_, T>,
        timeout: Duration,
    ) -> Result<PreviewResponse, PreviewBuffer> {
        (**self).prefetch(request, timeout)
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
/// let picker = Picker::new(render);
/// picker.push_batch([
///     Item { name: "red", description: "RGB: 255, 0, 0" },
///     Item { name: "blue", description: "RGB: 0, 0, 255" },
/// ]);
/// let previewer = SyncPreviewer(|item: &Item, buffer: &mut PreviewBuffer| {
///     buffer.push_text(item.description);
/// });
/// let mut picker = picker.with_preview(previewer);
/// if let Some(item) = picker.pick()? {
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
    T: Send + Sync + 'static,
    F: FnMut(&T, &mut PreviewBuffer),
{
    #[inline]
    fn preview(&mut self, request: PreviewRequest<'_, T>, _timeout: Duration) -> PreviewResponse {
        let item = request.item();
        let mut buffer = request.ready();
        (self.0)(item, &mut buffer);
        PreviewResponse::Ready(buffer)
    }
}
