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
//!   docs](CachedPreview#preview-cache).
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
//! # fn main() -> std::io::Result<()> {
//! # #[cfg(feature = "preview-cache")]
//! # {
//! use nucleo_picker::{
//!     Picker,
//!     interface::Multi,
//!     preview::{Pane, SyncPreviewer},
//!     render::StrRenderer,
//! };
//!
//! let picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//!
//! // this is a bit of a silly previewer, which just previews the item
//! // text itself, but it's enough for the example
//! let previewer = SyncPreviewer(|color: &&str, pane: &mut Pane| {
//!     pane.buffer_mut().push_text(color);
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
//! for color in picker.as_interface(Multi::new()).pick()? {
//!     println!("{color}");
//! }
//! # }
//! # Ok(())
//! # }
//! ```
//!
//! ### Extended example
//!
//! This example uses a stateful previewer. Run this example with
//! `cargo run --release --example preview_style --features preview-cache`.
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
#![doc = include_str!("../examples/preview_style.rs")]
//! # }
//! # fn main() {}
//! ```
//!
//! ### Background preview example
//!
//! Here is a basic example of a previewer running in a background thread. This also uses the
//! [`Worker`] trait, which is the same abstraction used by a [`PoolPreviewer`].
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
#![doc = include_str!("../examples/preview_single.rs")]
//! # }
//! # fn main() {}
//! ```

#[cfg(feature = "preview-ansi")]
mod ansi;
mod buffer;
mod draw;
#[cfg(feature = "preview-cache")]
pub(crate) mod lock;
mod pane;
#[cfg(feature = "preview-cache")]
mod pool;
#[cfg(feature = "preview-cache")]
mod provider;
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub mod request;
mod text;

use crate::rect::Dimensions;

#[cfg(feature = "preview-cache")]
#[doc(inline)]
pub use crate::interface::CachedPreview;
#[doc(inline)]
pub use crate::interface::UncachedPreview;
#[doc(inline)]
pub use crate::session::{ItemId, ItemRef};
#[cfg(feature = "preview-ansi")]
pub use ansi::{AnsiState, AnsiWriter};
pub use buffer::{Buffer, Line};
pub(crate) use draw::draw;
pub use pane::{Pane, Viewport};
#[cfg(feature = "preview-cache")]
pub use pool::{PoolPreviewer, Worker};
#[cfg(feature = "preview-cache")]
pub use provider::{Preview, SyncPreviewer};
pub use text::TextWriter;

/// Events which target the preview pane.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewEvent {
    /// Refresh the preview for the hovered item.
    Refresh,
    /// Scroll to the left by `n` columns.
    Left(usize),
    /// Scroll to the right by `n` columns.
    Right(usize),
    /// Align the preview window with the first column.
    AlignLeft,
    /// Align the right edge of the preview window with the longest currently visible row, or to
    /// the first column if all of the rows fit.
    ///
    /// Note that this may result in a left movement if the preview pane is currently scrolled past
    /// the final visible row. If you want to align to the right *without any possible left movement*,
    /// use `PreviewEvent::Right(usize::MAX)`.
    AlignRight,
    /// Scroll towards the beginning of the buffer by `n` lines.
    Up(usize),
    /// Scroll towards the end of the buffer by `n` lines.
    Down(usize),
    /// Scroll towards the beginning of the buffer by `n` pages.
    PageUp(usize),
    /// Scroll towards the end of the buffer by `n` pages.
    PageDown(usize),
    /// Align the preview window to the first row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignTop,
    /// Align the preview window to the final row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignBottom,
    /// Toggle the presence of line numbers in an active preview pane.
    ToggleLineNumbers,
    /// Set visibility of line numbers in the active preview pane.
    ///
    /// This event has no default keybinding.
    SetLineNumbers(bool),
}

/// The outcome of rendering a preview pane.
pub enum Content<'a> {
    /// A preview pane with no contents.
    Empty,
    /// A preview pane containing a loading indicator.
    Loading,
    /// A rendered preview pane (including preview panes rendering errors).
    Ready(&'a Pane),
}

pub(crate) fn content_dimensions(size: Dimensions, bordered: bool) -> Dimensions {
    let inset = if bordered { 2 } else { 0 };
    Dimensions {
        width: size.width.saturating_sub(inset),
        height: size.height.saturating_sub(inset),
    }
}

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
