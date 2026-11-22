use std::iter::once;

use crossterm::style::{Color, ContentStyle, Stylize};

use super::{BoundaryChars, Content, Pane, content_dimensions};
use crate::{
    draw::Surface,
    rect::Dimensions,
    util::{
        line::print_line,
        unicode::{AsciiProcessor, UnicodeProcessor},
    },
};

pub(crate) fn draw<S: Surface>(
    surface: &mut S,
    preview: Content<'_>,
    border: Option<BoundaryChars>,
    ellipsis: char,
) -> Result<(), S::Error> {
    if surface.is_empty() {
        return Ok(());
    }
    let width = surface.width();
    let height = surface.height();
    let content = content_dimensions(Dimensions { width, height }, border.is_some());
    let (preview, loading) = match preview {
        Content::Empty => (None, false),
        Content::Loading => (None, true),
        Content::Ready(pane) => (Some(pane), false),
    };
    let number_width = preview.map_or(0, |pane| pane.number_width(content.width));
    let (horizontal_position, _) = preview.map_or((0, 0), Pane::offset);
    let is_err = preview.is_some_and(Pane::is_err);
    let preview = preview.map(|pane| (pane.buffer(), pane.visible_rows(content.height).start));
    let border_style = ContentStyle {
        foreground_color: is_err.then_some(Color::DarkRed),
        ..ContentStyle::new()
    };
    if let Some(chars) = border {
        for (row, left, right) in [
            (0, chars.top_left, chars.top_right),
            (height - 1, chars.bottom_left, chars.bottom_right),
        ]
        .into_iter()
        .take(usize::from(height.min(2)))
        {
            surface.move_to(0, row)?;
            surface.text(left.encode_utf8(&mut [0; 4]), border_style)?;
            let mut remaining = content.width;
            if is_err && row == 0 && width >= 7 {
                let label = if width < 9 { " Err " } else { " Error " };
                if width >= 11 {
                    surface.text(chars.horizontal.encode_utf8(&mut [0; 4]), border_style)?;
                    remaining -= 1;
                }
                surface.text(label, border_style)?;
                remaining -= label.len() as u16;
            }
            let mut encoded = [0; 4];
            let horizontal = chars.horizontal.encode_utf8(&mut encoded);
            for _ in 0..remaining {
                surface.text(horizontal, border_style)?;
            }
            if width > 1 {
                surface.text(right.encode_utf8(&mut [0; 4]), border_style)?;
            }
        }
    }

    let inset = u16::from(border.is_some());
    for row in inset..height - inset {
        surface.move_to(0, row)?;
        if let Some(chars) = border {
            surface.text(chars.vertical.encode_utf8(&mut [0; 4]), border_style)?;
            if width == 1 {
                continue;
            }
        }
        let line = preview.and_then(|(buffer, start)| {
            let index = start + usize::from(row - inset);
            buffer.line(index).map(|line| (index + 1, line))
        });
        let remaining = if let Some((number, line)) = line {
            if number_width != 0 {
                let digits = usize::from(number_width - 1);
                surface.ascii(
                    format_args!("{number:>digits$}"),
                    ContentStyle::new().on_black(),
                )?;
                surface.spaces(1, ContentStyle::new())?;
            }
            let capacity = content.width - number_width;
            print_line::<UnicodeProcessor, _>(
                surface,
                line.as_str(),
                line.spans(),
                horizontal_position,
                capacity,
                ellipsis,
                ContentStyle::new(),
            )?
        } else if loading && row == inset {
            let message = "Loading...";
            let capacity = content.width;
            let style = ContentStyle::new().dark_grey();
            print_line::<AsciiProcessor, _>(
                surface,
                message,
                once(style.apply(message)),
                0,
                capacity,
                ellipsis,
                style,
            )?
        } else {
            content.width
        };
        surface.spaces(remaining, ContentStyle::new())?;
        if let Some(chars) = border {
            surface.text(chars.vertical.encode_utf8(&mut [0; 4]), border_style)?;
        }
    }
    Ok(())
}
