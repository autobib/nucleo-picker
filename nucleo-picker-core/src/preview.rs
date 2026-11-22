//! # Backend-agnostic preview components
//!
//! This module contains the backend-agnostic preview components.
//!
//! - The basic type is the [`Buffer`], which is essentially a vector of styled [lines](Line).
//! - The [`Buffer`] is wrapped in a [`Pane`], which adds line numbering, error state, and
//!   navigation state.
//! - A [`Pane`] can be wrapped in [`Content`] which indicates one of the three states a preview
//!   pane can be in: empty, loading, or ready.

#[cfg(feature = "preview-ansi")]
mod ansi;
mod buffer;
mod draw;
mod pane;
mod text;

use crate::rect::Dimensions;

#[cfg(feature = "preview-ansi")]
pub use ansi::{AnsiState, AnsiWriter};
pub use buffer::{Buffer, Line};
pub(crate) use draw::draw;
pub use pane::{Pane, Viewport};
pub use text::TextWriter;

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
/// Note that all characters must have Unicode width 1 or the terminal may be corrupted.
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
