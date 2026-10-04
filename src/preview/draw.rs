use std::io;

use crossterm::style::Color;

use super::{PreviewBuffer, PreviewLine};
use crate::{
    rect::Rect,
    util::unicode::{AsciiProcessor, Processor, UnicodeProcessor, is_ascii_safe, truncate},
};

pub(super) fn draw<D: Rect>(
    rect: &mut D,
    preview: Option<(&PreviewBuffer, usize)>,
    chars: [char; 6],
    ellipsis: char,
    line_numbers: bool,
) -> io::Result<()> {
    let width = rect.width().get();
    let height = rect.height().get();
    let [
        top_left,
        top_right,
        bottom_right,
        bottom_left,
        vertical,
        horizontal,
    ] = chars;
    for (row, left, right) in [
        (0, top_left, top_right),
        (height - 1, bottom_left, bottom_right),
    ] {
        rect.move_to(0, row)?;
        rect.clear_line()?;
        rect.print(left)?;
        for _ in 0..width - 2 {
            rect.print(horizontal)?;
        }
        rect.print(right)?;
    }

    let number_width = preview
        .filter(|_| line_numbers)
        .map_or(0, |(buffer, _)| buffer.lines().len().ilog10() as u16 + 2);
    let number_width = if number_width < width - 2 {
        number_width
    } else {
        0
    };
    for row in 1..height - 1 {
        rect.move_to(0, row)?;
        rect.clear_line()?;
        rect.print(vertical)?;
        let line = preview.and_then(|(buffer, start)| {
            let index = start + usize::from(row - 1);
            buffer.line(index).map(|line| (index + 1, line))
        });
        let remaining = if let Some((number, line)) = line {
            if number_width != 0 {
                rect.set_background(Color::DarkGrey)?;
                let digits = usize::from(number_width - 1);
                rect.print(format_args!("{number:>digits$} "))?;
                rect.reset_style()?;
            }
            let capacity = width - 2 - number_width;
            if is_ascii_safe(line.as_str()) {
                draw_line::<AsciiProcessor, _>(rect, line, capacity, ellipsis)?
            } else {
                draw_line::<UnicodeProcessor, _>(rect, line, capacity, ellipsis)?
            }
        } else {
            width - 2
        };
        rect.spaces(remaining)?;
        rect.print(vertical)?;
    }
    Ok(())
}

fn draw_line<P: Processor, D: Rect>(
    rect: &mut D,
    line: PreviewLine<'_>,
    capacity: u16,
    ellipsis: char,
) -> io::Result<u16> {
    let text = line.as_str();
    // Clip the complete line so style boundaries cannot split a grapheme for width calculations.
    let (prefix, remaining, markers) = match truncate::<P>(text, capacity) {
        Ok(remaining) => (text, remaining, 0),
        Err((prefix, 0)) => match truncate::<P>(prefix, capacity - 1) {
            Err((prefix, alignment)) => (prefix, 0, alignment + 1),
            Ok(remaining) => (prefix, 0, usize::from(remaining) + 1),
        },
        Err((prefix, alignment)) => (prefix, 0, alignment),
    };
    let mut bytes = prefix.len();
    for span in line.spans() {
        if bytes == 0 {
            break;
        }
        let content = span.content();
        let len = content.len().min(bytes);
        rect.print_styled(span.style().apply(&content[..len]))?;
        // Crossterm resets foreground, but not underline colour, for attribute-free spans.
        if span.style().underline_color.is_some() && span.style().attributes.is_empty() {
            rect.reset_style()?;
        }
        bytes -= len;
    }
    for _ in 0..markers {
        rect.print(ellipsis)?;
    }
    Ok(remaining)
}
