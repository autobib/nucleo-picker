//! # Ratatui interoperability

use std::{fmt, fmt::Write as _, num::NonZero};

use ratatui_core::{
    buffer::{Buffer, Cell, CellDiffOption, CellWidth},
    layout::Rect,
    style::{Color, Modifier, Style as RatatuiStyle},
    text::Span,
};

use super::{Attributes, Color as PickerColor, Style, StyledStr, Surface};
use crate::util::unicode::{Processor, UnicodeProcessor, grapheme_width};

/// A [`Surface`] adapter for Ratatui buffers.
///
/// This type is a thin wrapper around a [`Rect`] sub-area of a [`Buffer`] which permits the picker
/// to draw directly into the buffer.
pub struct RatatuiSurface<'a> {
    cells: &'a mut [Cell],
    stride: usize,
    width: u16,
    height: u16,
    column: u16,
    row: u16,
    check_left: bool,
}

impl<'a> RatatuiSurface<'a> {
    /// Initialize a rectangle from a Ratatui rectangle and buffer.
    ///
    /// # Panics
    ///
    /// The provided rectangle must be a subset of the buffer rectangle.
    pub fn new(buffer: &'a mut Buffer, area: Rect) -> Self {
        let bounds = buffer.area;
        assert!(
            area.x >= bounds.x
                && area.y >= bounds.y
                && u32::from(area.x) + u32::from(area.width)
                    <= u32::from(bounds.x) + u32::from(bounds.width)
                && u32::from(area.y) + u32::from(area.height)
                    <= u32::from(bounds.y) + u32::from(bounds.height),
            "drawing rectangle must be contained in the buffer"
        );
        let stride = usize::from(bounds.width);
        let cells = if area.is_empty() {
            &mut buffer.content[..0]
        } else {
            let start = usize::from(area.y - bounds.y) * stride + usize::from(area.x - bounds.x);
            let len = usize::from(area.height - 1) * stride + usize::from(area.width);
            &mut buffer.content[start..start + len]
        };
        Self {
            cells,
            stride,
            width: area.width,
            height: area.height,
            column: 0,
            row: 0,
            check_left: false,
        }
    }

    fn row_mut(&mut self) -> &mut [Cell] {
        assert!(
            self.width != 0 && self.row < self.height,
            "drawing cursor is invalid"
        );
        let start = usize::from(self.row) * self.stride;
        &mut self.cells[start..start + usize::from(self.width)]
    }

    fn write_cells(&mut self, width: u16) -> &mut [Cell] {
        let column = usize::from(self.column);
        let end = column + usize::from(width);
        assert!(
            end <= usize::from(self.width),
            "text must fit in the drawing rectangle"
        );
        let check_left = std::mem::take(&mut self.check_left);
        self.column = end as u16;
        let row = self.row_mut();

        // Sequential writes have already cleared intersecting glyphs. Only explicit movement
        // can put the pen inside the trailing cells of an existing wide glyph.
        if check_left {
            let mut start = 0;
            while start < column {
                let end = (start + usize::from(row[start].cell_width().max(1))).min(row.len());
                if end > column {
                    row[start..end].fill(Cell::EMPTY);
                }
                start = end;
            }
        }
        let mut start = column;
        while start < end {
            let next = (start + usize::from(row[start].cell_width().max(1))).min(row.len());
            row[start..next].fill(Cell::EMPTY);
            start = next;
        }
        &mut row[column..end]
    }

    fn write_text(&mut self, text: &str, mut style_at: impl FnMut(usize) -> RatatuiStyle) {
        assert!(
            text.is_empty() || (self.width != 0 && self.row < self.height),
            "drawing cursor is invalid"
        );
        let mut graphemes = UnicodeProcessor::grapheme_indices(text).peekable();
        let mut start = 0;
        while let Some((index, grapheme)) = graphemes.next() {
            let width = grapheme_width(grapheme);
            if width == 0 {
                continue;
            }
            while graphemes
                .peek()
                .is_some_and(|&(_, grapheme)| grapheme_width(grapheme) == 0)
            {
                graphemes.next();
            }
            let end = graphemes.peek().map_or(text.len(), |&(index, _)| index);
            let symbol = &text[start..end];
            let width = u16::try_from(width).expect("grapheme must fit in the drawing rectangle");
            let cell = &mut self.write_cells(width)[0];
            cell.set_symbol(symbol).set_style(style_at(index));
            // a cell can also contain adjacent zero-width graphemes
            if symbol.cell_width() != width {
                cell.set_diff_option(CellDiffOption::ForcedWidth(NonZero::new(width).unwrap()));
            }
            start = end;
        }
    }
}

impl Surface for RatatuiSurface<'_> {
    type Error = !;

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn move_to(&mut self, column: u16, row: u16) -> Result<(), Self::Error> {
        assert!(column < self.width && row < self.height);
        self.column = column;
        self.row = row;
        self.check_left = column != 0;
        Ok(())
    }

    fn move_to_column(&mut self, column: u16) -> Result<(), Self::Error> {
        self.move_to(column, self.row)
    }

    fn next_line(&mut self) -> Result<(), Self::Error> {
        assert!(
            self.width != 0 && self.row < self.height,
            "drawing cursor is invalid"
        );
        self.row += 1;
        self.column = 0;
        self.check_left = false;
        Ok(())
    }

    fn text(&mut self, text: &str, style: Style) -> Result<(), Self::Error> {
        if !text.is_empty() {
            let style = RatatuiStyle::from(style);
            self.write_text(text, |_| style);
        }
        Ok(())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        mut spans: impl Iterator<Item = StyledStr<'a>>,
    ) -> Result<(), Self::Error> {
        let mut end = 0;
        let mut style = RatatuiStyle::new();
        self.write_text(text, |index| {
            while end <= index {
                let span = spans.next().expect("spans must partition the text");
                end += span.text.len();
                style = span.style.into();
            }
            style
        });
        Ok(())
    }

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> Result<(), Self::Error> {
        struct Writer<'a, 'buffer> {
            surface: &'a mut RatatuiSurface<'buffer>,
            style: RatatuiStyle,
        }

        impl fmt::Write for Writer<'_, '_> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                if text.is_empty() {
                    return Ok(());
                }
                debug_assert!(text.bytes().all(|byte| (b' '..=b'~').contains(&byte)));
                let width = u16::try_from(text.len()).expect("ASCII must fit in the rectangle");
                for (cell, byte) in self.surface.write_cells(width).iter_mut().zip(text.bytes()) {
                    cell.set_char(char::from(byte)).set_style(self.style);
                }
                Ok(())
            }
        }

        Writer {
            surface: self,
            style: style.into(),
        }
        .write_fmt(text)
        .expect("ASCII formatter failed");
        Ok(())
    }

    fn spaces(&mut self, width: u16, style: Style) -> Result<(), Self::Error> {
        if width != 0 {
            let style = RatatuiStyle::from(style);
            for cell in self.write_cells(width) {
                cell.set_style(style);
            }
        }
        Ok(())
    }

    fn clear_line(&mut self) -> Result<(), Self::Error> {
        self.row_mut().fill(Cell::EMPTY);
        self.column = 0;
        self.check_left = false;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        if self.is_empty() {
            return Ok(());
        }
        for row in self.cells.chunks_mut(self.stride) {
            row[..usize::from(self.width)].fill(Cell::EMPTY);
        }
        self.move_to(0, 0)
    }
}

macro_rules! colors {
    ($($native:ident => $ratatui:ident),* $(,)?) => {
        impl From<PickerColor> for Color {
            #[inline]
            fn from(color: PickerColor) -> Self {
                match color {
                    $(PickerColor::$native => Self::$ratatui,)*
                    PickerColor::Rgb { r, g, b } => Self::Rgb(r, g, b),
                    PickerColor::AnsiValue(value) => Self::Indexed(value),
                }
            }
        }

        impl From<Color> for PickerColor {
            #[inline]
            fn from(color: Color) -> Self {
                match color {
                    $(Color::$ratatui => Self::$native,)*
                    Color::Rgb(r, g, b) => Self::Rgb { r, g, b },
                    Color::Indexed(value) => Self::AnsiValue(value),
                }
            }
        }
    };
}

colors! {
    Reset => Reset,
    Black => Black,
    DarkGrey => DarkGray,
    Red => LightRed,
    DarkRed => Red,
    Green => LightGreen,
    DarkGreen => Green,
    Yellow => LightYellow,
    DarkYellow => Yellow,
    Blue => LightBlue,
    DarkBlue => Blue,
    Magenta => LightMagenta,
    DarkMagenta => Magenta,
    Cyan => LightCyan,
    DarkCyan => Cyan,
    White => White,
    Grey => Gray,
}

macro_rules! attributes {
    ($($native:ident => $ratatui:ident),* $(,)?) => {
        impl From<Attributes> for Modifier {
            #[inline]
            fn from(attributes: Attributes) -> Self {
                let mut converted = Self::empty();
                $(
                    if attributes.contains(Attributes::$native) {
                        converted.insert(Self::$ratatui);
                    }
                )*
                if !(attributes
                    & (Attributes::DOUBLE_UNDERLINED
                        | Attributes::UNDERCURLED
                        | Attributes::UNDERDOTTED
                        | Attributes::UNDERDASHED))
                    .is_empty()
                {
                    converted.insert(Self::UNDERLINED);
                }
                converted
            }
        }

        impl From<Modifier> for Attributes {
            #[inline]
            fn from(modifier: Modifier) -> Self {
                let mut converted = Self::empty();
                $(
                    if modifier.contains(Modifier::$ratatui) {
                        converted.insert(Self::$native);
                    }
                )*
                converted
            }
        }
    };
}

attributes! {
    BOLD => BOLD,
    DIM => DIM,
    ITALIC => ITALIC,
    UNDERLINED => UNDERLINED,
    SLOW_BLINK => SLOW_BLINK,
    RAPID_BLINK => RAPID_BLINK,
    REVERSE => REVERSED,
    HIDDEN => HIDDEN,
    CROSSED_OUT => CROSSED_OUT,
}

impl From<Style> for RatatuiStyle {
    #[inline]
    fn from(style: Style) -> Self {
        Self {
            fg: style.foreground_color.map(Into::into),
            bg: style.background_color.map(Into::into),
            underline_color: style.underline_color.map(Into::into),
            add_modifier: style.attributes.into(),
            sub_modifier: Modifier::empty(),
        }
    }
}

impl From<RatatuiStyle> for Style {
    #[inline]
    fn from(style: RatatuiStyle) -> Self {
        Self {
            foreground_color: style.fg.map(Into::into),
            background_color: style.bg.map(Into::into),
            underline_color: style.underline_color.map(Into::into),
            attributes: (style.add_modifier - style.sub_modifier).into(),
        }
    }
}

impl<'a> From<StyledStr<'a>> for Span<'a> {
    #[inline]
    fn from(styled: StyledStr<'a>) -> Self {
        Self::styled(styled.text, styled.style)
    }
}

impl<'a> From<&'a Span<'_>> for StyledStr<'a> {
    #[inline]
    fn from(span: &'a Span<'_>) -> Self {
        Self {
            text: span.content.as_ref(),
            style: span.style.into(),
        }
    }
}

#[cfg(test)]
mod tests;
