use std::iter::once;

use crossterm::style::{Color, ContentStyle, Stylize};

use super::{BoundaryChars, Content, Pane};
use crate::{
    draw::Draw,
    util::{
        line::print_line,
        unicode::{AsciiProcessor, UnicodeProcessor},
    },
};

pub(crate) fn draw<D: Draw>(
    rect: &mut D,
    preview: Content<'_>,
    chars: BoundaryChars,
    ellipsis: char,
) -> Result<(), D::Error> {
    let (preview, loading) = match preview {
        Content::Empty => (None, false),
        Content::Loading => (None, true),
        Content::Ready(pane) => (Some(pane), false),
    };
    let number_width = preview.map_or(0, |pane| pane.number_width(rect.width().get() - 2));
    let (horizontal_position, _) = preview.map_or((0, 0), Pane::offset);
    let is_err = preview.is_some_and(Pane::is_err);
    let preview = preview.map(|pane| {
        (
            pane.buffer(),
            pane.visible_rows(rect.height().get() - 2).start,
        )
    });
    let border_style = ContentStyle {
        foreground_color: is_err.then_some(Color::DarkRed),
        ..ContentStyle::new()
    };
    let width = rect.width().get();
    let height = rect.height().get();
    let BoundaryChars {
        top_left,
        top_right,
        bottom_left,
        bottom_right,
        vertical,
        horizontal,
    } = chars;
    for (row, left, right) in [
        (0, top_left, top_right),
        (height - 1, bottom_left, bottom_right),
    ] {
        rect.move_to(0, row)?;
        rect.clear_line()?;
        rect.text(left.encode_utf8(&mut [0; 4]), border_style)?;
        let mut remaining = width - 2;
        if is_err && row == 0 && width >= 7 {
            let label = if width < 9 { " Err " } else { " Error " };
            if width >= 11 {
                rect.text(horizontal.encode_utf8(&mut [0; 4]), border_style)?;
                remaining -= 1;
            }
            rect.text(label, border_style)?;
            remaining -= label.len() as u16;
        }
        let mut encoded = [0; 4];
        let horizontal = horizontal.encode_utf8(&mut encoded);
        for _ in 0..remaining {
            rect.text(horizontal, border_style)?;
        }
        rect.text(right.encode_utf8(&mut [0; 4]), border_style)?;
    }

    for row in 1..height - 1 {
        rect.move_to(0, row)?;
        rect.clear_line()?;
        rect.text(vertical.encode_utf8(&mut [0; 4]), border_style)?;
        let line = preview.and_then(|(buffer, start)| {
            let index = start + usize::from(row - 1);
            buffer.line(index).map(|line| (index + 1, line))
        });
        let remaining = if let Some((number, line)) = line {
            if number_width != 0 {
                let digits = usize::from(number_width - 1);
                rect.ascii(
                    format_args!("{number:>digits$}"),
                    ContentStyle::new().on_black(),
                )?;
                rect.spaces(1, ContentStyle::new())?;
            }
            let capacity = width - 2 - number_width;
            print_line::<UnicodeProcessor, _>(
                rect,
                line.as_str(),
                line.spans(),
                horizontal_position,
                capacity,
                ellipsis,
                ContentStyle::new(),
            )?
        } else if loading && row == 1 {
            let message = "Loading...";
            let capacity = width - 2;
            let style = ContentStyle::new().dark_grey();
            print_line::<AsciiProcessor, _>(
                rect,
                message,
                once(style.apply(message)),
                0,
                capacity,
                ellipsis,
                style,
            )?
        } else {
            width - 2
        };
        rect.spaces(remaining, ContentStyle::new())?;
        rect.text(vertical.encode_utf8(&mut [0; 4]), border_style)?;
    }
    Ok(())
}
