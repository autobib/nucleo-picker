//! # Backend-agnostic drawing
//!
//! This module implements backend-agnostic drawing. The main entrypoint is the [`Draw`] trait.

use std::{fmt, num::NonZero};

use crossterm::style::{ContentStyle, StyledContent};

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
pub mod ratatui;

/// A non-empty surface for drawing in a single pass.
///
/// In order to reduce the number of calls required to actually draw, any draws to a rectangle must
/// uphold the following invariants:
///
/// 1. The content is assumed to be dirty. Any row clears *must precede* drawing that row, and
///    whole-rectangle clears *must precede* any writes.
/// 2. Ordinary writes may overwrite earlier writes.
/// 3. Text never wraps and is not clipped. The caller must ensure to not write text beyond the
///    boundary of the rectangle.
/// 4. Coordinates and writes must fit inside the rectangle.
/// 5. Text must not contain control characters or newlines. Graphemes must not span separate
///    text calls (but may span styles within a single [`styled_text`](Self::styled_text) call).
///
/// The columns and rows refer to local coordinates, indexed from `(0,0)` in the top-left corner.
pub trait Draw {
    /// An error which may occur while drawing.
    type Error;

    /// The width of the rectangle.
    fn width(&self) -> NonZero<u16>;

    /// The height of the rectangle.
    fn height(&self) -> NonZero<u16>;

    /// The number of terminal columns occupied by one extended grapheme cluster.
    ///
    /// The caller guarantees that the provided grapheme cluster does not contain any control
    /// characters or newlines.
    ///
    /// Implementations must return a width no greater than `grapheme.len()`.
    ///
    /// The default implementation uses [`unicode_width::UnicodeWidthStr`]. Override if the terminal
    /// has different width rules. Note that zero-width graphemes and graphemes with widths greater
    /// than 2 are permitted.
    ///
    /// This associated function defines the width policy for the backend, independently of a
    /// drawing surface. [`Viewport::with_grapheme_width`](crate::preview::Viewport::with_grapheme_width) uses
    /// the same policy for horizontal navigation.
    fn grapheme_width(grapheme: &str) -> usize {
        crate::util::unicode::grapheme_width(grapheme)
    }

    /// Move the cursor to the rectangle-local column and row.
    fn move_to(&mut self, column: u16, row: u16) -> Result<(), Self::Error>;

    /// Move the cursor to the rectangle-local column.
    fn move_to_column(&mut self, column: u16) -> Result<(), Self::Error>;

    /// Advance to the next row and put the cursor at column 0.
    ///
    /// Attempting to advance past the last row invalidates the cursor. The cursor must be reset
    /// with [`move_to`](Self::move_to) before continuing.
    fn next_line(&mut self) -> Result<(), Self::Error>;

    /// Print the given text at the cursor position.
    ///
    /// Note that the required preconditions described at the trait-level docs must be satisifed.
    fn text(&mut self, text: &str, style: ContentStyle) -> Result<(), Self::Error> {
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
        spans: impl Iterator<Item = StyledContent<&'a str>>,
    ) -> Result<(), Self::Error>;

    /// Print ASCII directly at the cursor position.
    ///
    /// The caller is required to guarantee that the formatted output fits and moreover does not
    /// contain control characters or non-ASCII characters.
    fn ascii(&mut self, text: fmt::Arguments<'_>, style: ContentStyle) -> Result<(), Self::Error>;

    /// Write some number of spaces in the given style.
    fn spaces(&mut self, width: u16, style: ContentStyle) -> Result<(), Self::Error>;

    /// Overwrite the current row with blanks and move the cursor to column 0.
    ///
    /// This function must be called *before any writes to the current line*.
    fn clear_line(&mut self) -> Result<(), Self::Error>;

    /// Overwrite the entire rectangle with blanks and move the cursor to the origin.
    ///
    /// This function must be called *before any writes to this rectangle*.
    fn clear(&mut self) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests;
