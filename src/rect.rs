//! Abstractions over rectangular regions in the TUI screen.
use std::{fmt, io, num::NonZero};

use crossterm::{
    QueueableCommand,
    cursor::{MoveTo, MoveToColumn},
    style::{
        Color, ContentStyle, ResetColor, SetAttributes, SetBackgroundColor, SetForegroundColor,
        SetUnderlineColor, StyledContent,
    },
    terminal::{Clear, ClearType},
};

use crate::draw::Draw;

/// The dimensions of a rectangle in the terminal, measured in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dimensions {
    /// The width of the rectangle in terminal columns.
    pub width: u16,
    /// The height of the rectangle in terminal columns.
    pub height: u16,
}

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
    /// The rectangle initially contains default-styled blank cells.
    Precleared,
    /// The rectangle may be dirty but is guaranteed to end at the right edge of the terminal.
    ToEndOfLine,
    /// Fallback for uncleared interior regions.
    WithinRect,
}

pub(crate) struct CrosstermTarget<'a, W: ?Sized> {
    writer: &'a mut W,
    style: Option<ContentStyle>,
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
    /// This returns `None` if the area is empty. Initialization does not emit any IO: the caller
    /// must position the cursor explicitly before performing any writes.
    pub fn rect(&mut self, area: Area, clear: ClearState) -> Option<CrosstermRect<'_, 'a, W>> {
        let width = NonZero::new(area.width)?;
        let height = NonZero::new(area.height)?;
        assert!(area.column.checked_add(area.width - 1).is_some());
        assert!(area.row.checked_add(area.height - 1).is_some());
        Some(CrosstermRect {
            target: self,
            origin: Position {
                column: area.column,
                row: area.row,
            },
            width,
            height,
            clear,
            row: 0,
        })
    }

    /// Reset all colours.
    pub fn reset_style(&mut self) -> io::Result<()> {
        self.set_style(ContentStyle::default())
    }

    /// Set a style attribute.
    fn set_style(&mut self, style: ContentStyle) -> io::Result<()> {
        if self.style == Some(style) {
            return Ok(());
        }
        let mut previous = self.style.unwrap_or_default();
        if self.style.is_none() || previous.attributes & style.attributes != previous.attributes {
            self.writer.queue(ResetColor)?;
            previous = ContentStyle::default();
        }
        if previous.background_color != style.background_color {
            self.writer.queue(SetBackgroundColor(
                style.background_color.unwrap_or(Color::Reset),
            ))?;
        }
        if previous.foreground_color != style.foreground_color {
            self.writer.queue(SetForegroundColor(
                style.foreground_color.unwrap_or(Color::Reset),
            ))?;
        }
        if previous.underline_color != style.underline_color {
            self.writer.queue(SetUnderlineColor(
                style.underline_color.unwrap_or(Color::Reset),
            ))?;
        }
        if previous.attributes != style.attributes {
            self.writer
                .queue(SetAttributes(style.attributes ^ previous.attributes))?;
        }
        self.style = Some(style);
        Ok(())
    }
}

/// An implementation of [`Draw`] using the crossterm backend to write bytes to an internal IO
/// stream.
pub(crate) struct CrosstermRect<'a, 'writer, W: ?Sized> {
    target: &'a mut CrosstermTarget<'writer, W>,
    origin: Position,
    width: NonZero<u16>,
    height: NonZero<u16>,
    clear: ClearState,
    row: u16,
}

impl<W: io::Write + ?Sized> Draw for CrosstermRect<'_, '_, W> {
    type Error = io::Error;

    fn width(&self) -> NonZero<u16> {
        self.width
    }

    fn height(&self) -> NonZero<u16> {
        self.height
    }

    fn move_to(&mut self, column: u16, row: u16) -> io::Result<()> {
        debug_assert!(column < self.width().get() && row < self.height().get());
        self.target
            .writer
            .queue(MoveTo(self.origin.column + column, self.origin.row + row))?;
        self.row = row;
        Ok(())
    }

    fn move_to_column(&mut self, column: u16) -> io::Result<()> {
        debug_assert!(column < self.width().get() && self.row < self.height().get());
        self.target
            .writer
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

    fn text(&mut self, text: &str, style: ContentStyle) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        debug_assert!(self.row < self.height.get());
        self.target.set_style(style)?;
        self.target.writer.write_all(text.as_bytes())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        spans: impl Iterator<Item = StyledContent<&'a str>>,
    ) -> io::Result<()> {
        if !text.is_empty() {
            for span in spans {
                self.text(span.content(), *span.style())?;
            }
        }
        Ok(())
    }

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: ContentStyle) -> io::Result<()> {
        if let Some(text) = text.as_str() {
            return self.text(text, style);
        }
        debug_assert!(self.row < self.height.get());
        self.target.set_style(style)?;
        self.target.writer.write_fmt(text)
    }

    fn spaces(&mut self, width: u16, style: ContentStyle) -> io::Result<()> {
        if width != 0 {
            debug_assert!(self.row < self.height.get());
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
                crate::util::write_spaces(self.target.writer, usize::from(self.width.get()))?;
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
