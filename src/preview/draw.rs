use std::{io, iter::once};

use crossterm::style::{Color, ContentStyle};

use super::{
    BoundaryChars,
    cache::{Cached, RequestState},
};
use crate::{
    rect::Rect,
    util::{
        line::print_line,
        unicode::{AsciiProcessor, UnicodeProcessor},
    },
};

pub(super) fn draw<D: Rect>(
    rect: &mut D,
    preview: Option<&Cached>,
    chars: BoundaryChars,
    ellipsis: char,
    line_numbers: bool,
) -> io::Result<()> {
    let pending =
        preview.is_some_and(|cached| matches!(cached.state, Some(RequestState::Pending(_))));
    let preview = preview.and_then(|cached| match &cached.state {
        Some(RequestState::Ready(buffer)) => Some((buffer, cached.scroll_position)),
        _ => None,
    });
    let is_err = preview.is_some_and(|(buffer, _)| buffer.is_err());
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
        if is_err {
            rect.set_foreground(Color::DarkRed)?;
        }
        rect.print(left)?;
        let mut remaining = width - 2;
        if is_err && row == 0 && width >= 7 {
            let label = if width < 9 { " Err " } else { " Error " };
            if width >= 11 {
                rect.print(horizontal)?;
                remaining -= 1;
            }
            rect.print(label)?;
            remaining -= label.len() as u16;
        }
        for _ in 0..remaining {
            rect.print(horizontal)?;
        }
        rect.print(right)?;
        if is_err {
            rect.reset_style()?;
        }
    }

    let number_width = preview
        .filter(|_| line_numbers)
        .map_or(0, |(buffer, _)| buffer.lines().len().ilog10() as u16 + 2);
    let number_width = if number_width < width - 3 {
        number_width
    } else {
        0
    };
    for row in 1..height - 1 {
        rect.move_to(0, row)?;
        rect.clear_line()?;
        rect.print_styled(border_style.apply(vertical))?;
        let line = preview.and_then(|(buffer, start)| {
            let index = start + usize::from(row - 1);
            buffer.line(index).map(|line| (index + 1, line))
        });
        let remaining = if let Some((number, line)) = line {
            if number_width != 0 {
                let digits = usize::from(number_width - 1);
                rect.set_background(Color::Black)?;
                rect.print(format_args!("{number:>digits$}"))?;
                rect.reset_style()?;
                rect.spaces(1)?;
            }
            let capacity = width - 2 - number_width;
            print_line::<UnicodeProcessor, _>(
                rect,
                line.as_str(),
                line.spans(),
                0,
                capacity,
                ellipsis,
            )?
        } else if pending && row == 1 {
            let message = "Loading...";
            let capacity = width - 2;
            rect.set_foreground(Color::DarkGrey)?;
            let remaining = print_line::<AsciiProcessor, _>(
                rect,
                message,
                once(ContentStyle::new().apply(message)),
                0,
                capacity,
                ellipsis,
            )?;
            rect.reset_style()?;
            remaining
        } else {
            width - 2
        };
        rect.spaces(remaining)?;
        rect.print_styled(border_style.apply(vertical))?;
    }
    Ok(())
}
