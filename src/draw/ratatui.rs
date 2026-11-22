//! # Ratatui interoperability

use std::{fmt, fmt::Write as _, num::NonZero};

use crossterm::style::{Attribute, Color as CrosstermColor, ContentStyle, StyledContent};
use ratatui_core::{
    buffer::{Buffer, Cell, CellDiffOption, CellWidth},
    layout::Rect,
    style::{Color, Modifier, Style},
};

use super::Surface;
use crate::util::unicode::{Processor, UnicodeProcessor, grapheme_width};

/// # [`Surface`] adapter for Ratatui buffers.
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
                let end = start + usize::from(row[start].cell_width().max(1));
                assert!(
                    end <= row.len(),
                    "wide glyph crosses the rectangle boundary"
                );
                if end > column {
                    row[start..end].fill(Cell::EMPTY);
                }
                start = end;
            }
        }
        let mut start = column;
        while start < end {
            let next = start + usize::from(row[start].cell_width().max(1));
            assert!(
                next <= row.len(),
                "wide glyph crosses the rectangle boundary"
            );
            row[start..next].fill(Cell::EMPTY);
            start = next;
        }
        &mut row[column..end]
    }

    fn write_text(&mut self, text: &str, mut style_at: impl FnMut(usize) -> Style) {
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

    fn text(&mut self, text: &str, style: ContentStyle) -> Result<(), Self::Error> {
        if !text.is_empty() {
            let style = convert_style(style);
            self.write_text(text, |_| style);
        }
        Ok(())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        mut spans: impl Iterator<Item = StyledContent<&'a str>>,
    ) -> Result<(), Self::Error> {
        let mut end = 0;
        let mut style = Style::new();
        self.write_text(text, |index| {
            while end <= index {
                let span = spans.next().expect("spans must partition the text");
                end += span.content().len();
                style = convert_style(*span.style());
            }
            style
        });
        Ok(())
    }

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: ContentStyle) -> Result<(), Self::Error> {
        struct Writer<'a, 'buffer> {
            surface: &'a mut RatatuiSurface<'buffer>,
            style: Style,
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
            style: convert_style(style),
        }
        .write_fmt(text)
        .expect("ASCII formatter failed");
        Ok(())
    }

    fn spaces(&mut self, width: u16, style: ContentStyle) -> Result<(), Self::Error> {
        if width != 0 {
            let style = convert_style(style);
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

fn convert_style(style: ContentStyle) -> Style {
    let attributes = style.attributes;
    let mut add_modifier = Modifier::empty();
    if attributes.has(Attribute::Bold) {
        add_modifier |= Modifier::BOLD;
    }
    if attributes.has(Attribute::Dim) {
        add_modifier |= Modifier::DIM;
    }
    if attributes.has(Attribute::Italic) {
        add_modifier |= Modifier::ITALIC;
    }
    if attributes.has(Attribute::Underlined)
        || attributes.has(Attribute::DoubleUnderlined)
        || attributes.has(Attribute::Undercurled)
        || attributes.has(Attribute::Underdotted)
        || attributes.has(Attribute::Underdashed)
    {
        add_modifier |= Modifier::UNDERLINED;
    }
    if attributes.has(Attribute::SlowBlink) {
        add_modifier |= Modifier::SLOW_BLINK;
    }
    if attributes.has(Attribute::RapidBlink) {
        add_modifier |= Modifier::RAPID_BLINK;
    }
    if attributes.has(Attribute::Reverse) {
        add_modifier |= Modifier::REVERSED;
    }
    if attributes.has(Attribute::Hidden) {
        add_modifier |= Modifier::HIDDEN;
    }
    if attributes.has(Attribute::CrossedOut) {
        add_modifier |= Modifier::CROSSED_OUT;
    }

    let mut sub_modifier = Modifier::empty();
    if attributes.has(Attribute::NoBold) {
        sub_modifier |= Modifier::BOLD;
    }
    if attributes.has(Attribute::NoItalic) {
        sub_modifier |= Modifier::ITALIC;
    }
    if attributes.has(Attribute::NotCrossedOut) {
        sub_modifier |= Modifier::CROSSED_OUT;
    }
    if attributes.has(Attribute::NoUnderline) {
        sub_modifier |= Modifier::UNDERLINED;
    }
    if attributes.has(Attribute::NoHidden) {
        sub_modifier |= Modifier::HIDDEN;
    }
    if attributes.has(Attribute::NoBlink) {
        sub_modifier |= Modifier::RAPID_BLINK | Modifier::SLOW_BLINK;
    }
    if attributes.has(Attribute::NoReverse) {
        sub_modifier |= Modifier::REVERSED;
    }

    let mut converted = Style {
        fg: style.foreground_color.map(convert_color),
        bg: style.background_color.map(convert_color),
        underline_color: style.underline_color.map(convert_color),
        add_modifier,
        sub_modifier,
    };
    // ratatui-crossterm 0.1.2 omits NormalIntensity, which clears both bold and dim.
    // see https://github.com/ratatui/ratatui/issues/2920
    if style.attributes.has(Attribute::NormalIntensity) {
        converted = converted.remove_modifier(Modifier::BOLD | Modifier::DIM);
    }
    converted
}

fn convert_color(color: CrosstermColor) -> Color {
    match color {
        CrosstermColor::Reset => Color::Reset,
        CrosstermColor::Black => Color::Black,
        CrosstermColor::DarkRed => Color::Red,
        CrosstermColor::DarkGreen => Color::Green,
        CrosstermColor::DarkYellow => Color::Yellow,
        CrosstermColor::DarkBlue => Color::Blue,
        CrosstermColor::DarkMagenta => Color::Magenta,
        CrosstermColor::DarkCyan => Color::Cyan,
        CrosstermColor::Grey => Color::Gray,
        CrosstermColor::DarkGrey => Color::DarkGray,
        CrosstermColor::Red => Color::LightRed,
        CrosstermColor::Green => Color::LightGreen,
        CrosstermColor::Blue => Color::LightBlue,
        CrosstermColor::Yellow => Color::LightYellow,
        CrosstermColor::Magenta => Color::LightMagenta,
        CrosstermColor::Cyan => Color::LightCyan,
        CrosstermColor::White => Color::White,
        CrosstermColor::Rgb { r, g, b } => Color::Rgb(r, g, b),
        CrosstermColor::AnsiValue(value) => Color::Indexed(value),
    }
}

#[cfg(test)]
mod tests;
