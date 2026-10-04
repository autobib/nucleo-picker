//! Abstractions over rectangular regions in the TUI screen.
use std::{fmt::Display, io, num::NonZero};

use crossterm::{
    QueueableCommand,
    cursor::{MoveTo, MoveToColumn},
    style::{
        Attribute, Color, Print, PrintStyledContent, ResetColor, SetAttribute, SetBackgroundColor,
        SetForegroundColor, StyledContent,
    },
    terminal::{Clear, ClearType},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Position {
    pub column: u16,
    pub row: u16,
}

/// The area occupied by the rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Area {
    pub column: u16,
    pub row: u16,
    pub width: u16,
    pub height: u16,
}

impl Area {
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A clear policy fixed at the beginning of a repaint pass. The policy is based on any information
/// about the contents of the rectangle *at the beginning only*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClearState {
    Precleared,
    ToEndOfLine,
    WithinRect,
}

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
///
/// The columns and rows refer to local coordinates, indexed from `(0,0)` in the top-left corner.
pub(crate) trait Rect {
    /// The width of the rectangle.
    fn width(&self) -> NonZero<u16>;

    /// The height of the rectangle.
    fn height(&self) -> NonZero<u16>;

    /// Move the cursor to the rectangle-local column and row.
    fn move_to(&mut self, column: u16, row: u16) -> io::Result<()>;

    /// Move the cursor to the rectangle-local column.
    fn move_to_column(&mut self, column: u16) -> io::Result<()>;

    /// Advance to the next row and put the cursor at column 0.
    ///
    /// Attempting to advance past the last row does not result in cursor movement.
    fn next_line(&mut self) -> io::Result<()>;

    /// Print the given text at the cursor position.
    fn print(&mut self, text: impl Display) -> io::Result<()>;

    /// Print the given text at the cursor position with the provided style.
    fn print_styled<D: Display>(&mut self, text: StyledContent<D>) -> io::Result<()>;

    /// Set a style attribute.
    fn set_attribute(&mut self, attribute: Attribute) -> io::Result<()>;

    /// Set the foreground colour.
    fn set_foreground(&mut self, color: Color) -> io::Result<()>;

    /// Set the background colour.
    fn set_background(&mut self, color: Color) -> io::Result<()>;

    /// Reset all colours.
    fn reset_style(&mut self) -> io::Result<()>;

    /// Write some number of spaces.
    fn spaces(&mut self, width: u16) -> io::Result<()>;

    /// Overwrite the current row with blanks and move the cursor to column 0.
    ///
    /// This function must be called *before any writes to the current line*.
    fn clear_line(&mut self) -> io::Result<()>;

    /// Overwrite the entire rectangle with blanks and move the cursor to the origin.
    ///
    /// This function must be called *before any writes to this rectangle*.
    fn clear(&mut self) -> io::Result<()>;
}

/// An implementation of a [`Rect`] using the crossterm backend to write bytes to an internal IO
/// stream.
pub(crate) struct CrosstermRect<'a, W: ?Sized> {
    writer: &'a mut W,
    origin: Position,
    width: NonZero<u16>,
    height: NonZero<u16>,
    clear: ClearState,
    row: u16,
}

impl<'a, W: io::Write + ?Sized> CrosstermRect<'a, W> {
    /// Initialize the rectangle.
    ///
    /// This returns `None` if the area is empty.
    pub fn new(writer: &'a mut W, area: Area, clear: ClearState) -> Option<Self> {
        Some(Self {
            writer,
            origin: Position {
                column: area.column,
                row: area.row,
            },
            width: NonZero::new(area.width)?,
            height: NonZero::new(area.height)?,
            clear,
            row: 0,
        })
    }
}

impl<W: io::Write + ?Sized> Rect for CrosstermRect<'_, W> {
    fn width(&self) -> NonZero<u16> {
        self.width
    }

    fn height(&self) -> NonZero<u16> {
        self.height
    }

    fn move_to(&mut self, column: u16, row: u16) -> io::Result<()> {
        debug_assert!(column < self.width().get() && row < self.height().get());
        self.writer
            .queue(MoveTo(self.origin.column + column, self.origin.row + row))?;
        self.row = row;
        Ok(())
    }

    fn move_to_column(&mut self, column: u16) -> io::Result<()> {
        debug_assert!(column < self.width().get() && self.row < self.height().get());
        self.writer
            .queue(MoveToColumn(self.origin.column + column))?;
        Ok(())
    }

    fn next_line(&mut self) -> io::Result<()> {
        debug_assert!(self.row < self.height().get());
        let row = self.row + 1;
        if row < self.height().get() {
            self.move_to(0, row)?;
        } else {
            self.row = row;
        }
        Ok(())
    }

    fn print(&mut self, text: impl Display) -> io::Result<()> {
        self.writer.queue(Print(text))?;
        Ok(())
    }

    fn print_styled<D: Display>(&mut self, text: StyledContent<D>) -> io::Result<()> {
        self.writer.queue(PrintStyledContent(text))?;
        Ok(())
    }

    fn set_attribute(&mut self, attribute: Attribute) -> io::Result<()> {
        self.writer.queue(SetAttribute(attribute))?;
        Ok(())
    }

    fn set_foreground(&mut self, color: Color) -> io::Result<()> {
        self.writer.queue(SetForegroundColor(color))?;
        Ok(())
    }

    fn set_background(&mut self, color: Color) -> io::Result<()> {
        self.writer.queue(SetBackgroundColor(color))?;
        Ok(())
    }

    fn reset_style(&mut self) -> io::Result<()> {
        self.writer
            .queue(ResetColor)?
            .queue(SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    fn spaces(&mut self, width: u16) -> io::Result<()> {
        if width != 0 {
            crate::util::write_spaces(self.writer, usize::from(width))?;
        }
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.move_to_column(0)?;
        match self.clear {
            ClearState::Precleared => {}
            ClearState::ToEndOfLine => {
                self.writer.queue(Clear(ClearType::UntilNewLine))?;
            }
            ClearState::WithinRect => {
                crate::util::write_spaces(self.writer, usize::from(self.width.get()))?;
                self.move_to_column(0)?;
            }
        }
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        if self.clear != ClearState::Precleared {
            for row in 0..self.height().get() {
                self.move_to(0, row)?;
                self.clear_line()?;
            }
        }
        self.move_to(0, 0)
    }
}

#[cfg(test)]
mod tests;
