//! # Backend-agnostic drawing
//!
//! This module implements backend-agnostic drawing.
//!
//! - The [`Surface`] trait represents a surface against which a component can be drawn.
//! - [`Color`], [`Attributes`], [`Style`], and [`StyledStr`] are style types for text.
//!
//! ## Conversions
//!
//! With the `crossterm` feature enabled, the text style types have conversions to and from
//! crossterm types. Unlike `crossterm` types, the attribute type here does not include resets
//! since styles are, essentially, declarative, in that they describe *what some text looks like*
//! rather than the underlying commands which convert between style and ANSI codes. This means the
//! conversion from `crossterm` types will drop style resets.
//!
//! With the `ratatui` feature enabled, one gets [`RatatuiSurface`], which is an adapter for a
//! Ratatui buffer that implements [`Surface`]. There are also conversions to and from Ratatui
//! types. Similarly to crossterm, the conversions are somewhat lossy: extended underline attributes
//! are dropped and attributes here not supported by Ratatui are simply dropped.

use std::fmt;

mod attributes;
mod color;
#[cfg(feature = "crossterm")]
mod crossterm;
#[cfg(feature = "ratatui")]
mod ratatui;
mod style;

pub use attributes::Attributes;
pub use color::Color;
pub use style::{Style, StyledStr};

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
pub use ratatui::RatatuiSurface;

/// A surface for drawing in a single pass.
///
/// In order to reduce the number of calls required to actually draw, any draws to a rectangle must
/// uphold the following invariants:
///
/// 1. Content must be written one row at a time by calling [`begin_row`](Surface::begin_row)
///    before writing. Rows must be written in strictly increasing order and contiguously from
///    column zero without overwriting. If `clear = true` the row is assumed to be blank, and
///    otherwise the contents of the row are unspecified and the caller must overwrite the entire
///    row.
/// 2. Text never wraps and is not clipped. The caller must ensure to not write text beyond the
///    boundary of the rectangle.
/// 3. Coordinates and writes must fit inside the rectangle.
/// 4. Text must not contain control characters or newlines. Graphemes must not span separate
///    text calls (but may span styles within a single [`styled_text`](Self::styled_text) call).
///
/// The columns and rows refer to local coordinates, indexed from `(0,0)` in the top-left corner.
/// Text widths are measured by summing the value of `unicode-width` over all extended grapheme
/// clusters. In particular, this may be a bit different than measuring the width of the entire
/// string in one go.
pub trait Surface {
    /// An error which may occur while drawing.
    type Error;

    /// The width of the rectangle.
    fn width(&self) -> u16;

    /// The height of the rectangle.
    fn height(&self) -> u16;

    /// Returns whether the surface is empty.
    ///
    /// By default, the surface is empty if either its width or height are 0.
    fn is_empty(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    /// Begin writing a row at column zero.
    ///
    /// Rows must be written in strictly increasing order. The `clear` bool requests if the row
    /// should be cleared before writing starts; if `clear = false` the contents of the row are
    /// unspecified.
    fn begin_row(&mut self, row: u16, clear: bool) -> Result<(), Self::Error>;

    /// Print the given text at the cursor position.
    ///
    /// Note that the required preconditions described at the trait-level docs must be satisfied.
    fn text(&mut self, text: &str, style: Style) -> Result<(), Self::Error> {
        if text.is_empty() {
            return Ok(());
        }
        self.styled_text(text, std::iter::once(style.apply(text)))
    }

    /// Print the given text at the cursor position with the provided style.
    ///
    /// The caller is required to guarantee that the provided spans exactly partition the provided
    /// `text`. Styled content spans may split graphemes.
    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        spans: impl Iterator<Item = StyledStr<'a>>,
    ) -> Result<(), Self::Error>;

    /// Print ASCII directly at the cursor position.
    ///
    /// The caller is required to guarantee that the formatted output fits and moreover does not
    /// contain control characters or non-ASCII characters.
    fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> Result<(), Self::Error>;

    /// Write some number of spaces in the given style.
    fn spaces(&mut self, width: u16, style: Style) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests;
