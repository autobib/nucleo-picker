//! Abstractions over rectangular regions in the TUI screen.
use std::{fmt, io};

use crossterm::{
    QueueableCommand,
    cursor::{MoveTo, MoveToColumn},
    style::{ResetColor, SetAttributes, SetBackgroundColor, SetForegroundColor, SetUnderlineColor},
    terminal::{Clear, ClearType},
};

use crate::draw::{Color, Style, StyledStr, Surface};

/// The dimensions of a rectangle in the terminal, measured in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dimensions {
    /// The width of the rectangle in terminal columns.
    pub width: u16,
    /// The height of the rectangle in terminal columns.
    pub height: u16,
}

/// A zero-based cursor position relative to a rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// The zero-based terminal column (0 being at the top).
    pub column: u16,
    /// The zero-based terminal row (0 being at the left).
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
    /// Returns if the area is empty, i.e. either the width or the height are 0.
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A clear policy fixed at the beginning of a repaint pass. The policy is based on any information
/// about the contents of the rectangle *at the beginning only*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClearState {
    /// The rectangle initially contains default-styled blank cells.
    Precleared,
    /// The rectangle may be dirty but is guaranteed to end at the right edge of the terminal.
    ToEndOfLine,
    /// Fallback for uncleared interior regions.
    WithinRect,
}

pub(crate) struct CrosstermTarget<'a, W: ?Sized> {
    writer: &'a mut W,
    style: Option<Style>,
}

impl<'a, W: io::Write + ?Sized> CrosstermTarget<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            style: None,
        }
    }

    /// Initialize the rectangle.
    ///
    /// Initialization does not perform any IO: the caller
    /// must position the cursor explicitly before performing any writes.
    ///
    /// # Panics
    ///
    /// This method will panic if the row and column limits (column plus width and row plus height) do not fit in a `u16`.
    pub fn rect(&mut self, area: Area, clear: ClearState) -> CrosstermSurface<'_, 'a, W> {
        assert!(area.column.checked_add(area.width).is_some());
        assert!(area.row.checked_add(area.height).is_some());
        CrosstermSurface {
            target: self,
            origin: Position {
                column: area.column,
                row: area.row,
            },
            width: area.width,
            height: area.height,
            clear,
            row: 0,
        }
    }

    /// Reset all colours.
    pub fn reset_style(&mut self) -> io::Result<()> {
        self.set_style(Style::default())
    }

    /// Set a style attribute.
    fn set_style(&mut self, style: Style) -> io::Result<()> {
        if self.style == Some(style) {
            return Ok(());
        }
        let mut previous = self.style.unwrap_or_default();
        if self.style.is_none() || !style.attributes.contains(previous.attributes) {
            self.writer.queue(ResetColor)?;
            previous = Style::default();
        }
        if previous.background_color != style.background_color {
            self.writer.queue(SetBackgroundColor(
                style.background_color.unwrap_or(Color::Reset).into(),
            ))?;
        }
        if previous.foreground_color != style.foreground_color {
            self.writer.queue(SetForegroundColor(
                style.foreground_color.unwrap_or(Color::Reset).into(),
            ))?;
        }
        if previous.underline_color != style.underline_color {
            self.writer.queue(SetUnderlineColor(
                style.underline_color.unwrap_or(Color::Reset).into(),
            ))?;
        }
        if previous.attributes != style.attributes {
            // competing attribute modes mean we have to re-apply the full set in order
            self.writer.queue(SetAttributes(style.attributes.into()))?;
        }
        self.style = Some(style);
        Ok(())
    }
}

/// An implementation of [`Surface`] using the crossterm backend to write bytes to an internal IO
/// stream.
pub(crate) struct CrosstermSurface<'a, 'writer, W: ?Sized> {
    target: &'a mut CrosstermTarget<'writer, W>,
    origin: Position,
    width: u16,
    height: u16,
    clear: ClearState,
    row: u16,
}

impl<W: io::Write + ?Sized> Surface for CrosstermSurface<'_, '_, W> {
    type Error = io::Error;

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn move_to(&mut self, column: u16, row: u16) -> io::Result<()> {
        debug_assert!(column < self.width() && row < self.height());
        self.target
            .writer
            .queue(MoveTo(self.origin.column + column, self.origin.row + row))?;
        self.row = row;
        Ok(())
    }

    fn move_to_column(&mut self, column: u16) -> io::Result<()> {
        debug_assert!(column < self.width() && self.row < self.height());
        self.target
            .writer
            .queue(MoveToColumn(self.origin.column + column))?;
        Ok(())
    }

    fn next_line(&mut self) -> io::Result<()> {
        debug_assert!(self.width != 0 && self.row < self.height());
        let row = self.row + 1;
        if row < self.height() {
            self.move_to(0, row)?;
        } else {
            self.row = row;
        }
        Ok(())
    }

    fn text(&mut self, text: &str, style: Style) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        debug_assert!(self.width != 0 && self.row < self.height);
        self.target.set_style(style)?;
        self.target.writer.write_all(text.as_bytes())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        spans: impl Iterator<Item = StyledStr<'a>>,
    ) -> io::Result<()> {
        if !text.is_empty() {
            for span in spans {
                self.text(span.text, span.style)?;
            }
        }
        Ok(())
    }

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> io::Result<()> {
        if let Some(text) = text.as_str() {
            return self.text(text, style);
        }
        debug_assert!(self.width != 0 && self.row < self.height);
        self.target.set_style(style)?;
        self.target.writer.write_fmt(text)
    }

    fn spaces(&mut self, width: u16, style: Style) -> io::Result<()> {
        if width != 0 {
            debug_assert!(self.width != 0 && self.row < self.height);
            self.target.set_style(style)?;
            crate::util::write_spaces(self.target.writer, usize::from(width))?;
        }
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.move_to_column(0)?;
        match self.clear {
            ClearState::Precleared => {}
            ClearState::ToEndOfLine => {
                self.target.reset_style()?;
                self.target.writer.queue(Clear(ClearType::UntilNewLine))?;
            }
            ClearState::WithinRect => {
                self.target.reset_style()?;
                crate::util::write_spaces(self.target.writer, usize::from(self.width))?;
                self.move_to_column(0)?;
            }
        }
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.clear != ClearState::Precleared {
            for row in 0..self.height() {
                self.move_to(0, row)?;
                self.clear_line()?;
            }
        }
        self.move_to(0, 0)
    }
}

#[cfg(test)]
mod tests;
