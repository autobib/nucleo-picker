use std::{
    io::{self, Write},
    num::NonZero,
};

use nucleo as nc;
use unicode_width::UnicodeWidthChar;

use super::{
    IndexBuffer, MatchList, MatchListConfig,
    item::RenderedItem,
    span::{Head, KeepLines, Spanned, Tail},
    unicode::{AsciiProcessor, UnicodeProcessor},
};
use crate::{
    PickerChars, Render,
    frame::ClearMode,
    util::{as_u16, write_spaces},
};

use crossterm::{
    QueueableCommand,
    cursor::MoveToNextLine,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};

/// The inner `match draw` implementation.
#[inline]
#[allow(clippy::too_many_arguments)]
fn draw_single_match<
    T: Send + Sync + 'static,
    R: Render<T>,
    L: KeepLines,
    W: Write + ?Sized,
    const SELECTED: bool,
>(
    writer: &mut W,
    buffer: &mut IndexBuffer,
    width: u16,
    clear_mode: ClearMode,
    config: &MatchListConfig,
    item: nc::Item<'_, T>,
    queued: bool,
    snapshot: &nc::Snapshot<T>,
    matcher: &mut nc::Matcher,
    height: u16,
    render: &R,
    chars: &PickerChars,
) -> io::Result<()> {
    // generate the indices
    if config.highlight {
        buffer.indices.clear();
        snapshot.pattern().column_pattern(0).indices(
            item.matcher_columns[0].slice(..),
            matcher,
            &mut buffer.indices,
        );
        buffer.indices.sort_unstable();
        buffer.indices.dedup();
    }

    match RenderedItem::new(&item, render) {
        RenderedItem::Ascii(s) => Spanned::<'_, AsciiProcessor>::new(
            &buffer.indices,
            s,
            &mut buffer.spans,
            &mut buffer.lines,
            L::from_offset(height),
        )
        .queue_print(
            writer,
            SELECTED,
            queued,
            width,
            config.highlight_padding,
            config.highlight_line,
            clear_mode,
            chars,
        ),
        RenderedItem::Unicode(r) => Spanned::<'_, UnicodeProcessor>::new(
            &buffer.indices,
            r.as_ref(),
            &mut buffer.spans,
            &mut buffer.lines,
            L::from_offset(height),
        )
        .queue_print(
            writer,
            SELECTED,
            queued,
            width,
            config.highlight_padding,
            config.highlight_line,
            clear_mode,
            chars,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_matches<'a, T: Send + Sync + 'static, R: Render<T>, W: io::Write + ?Sized>(
    writer: &mut W,
    buffer: &mut IndexBuffer,
    config: &MatchListConfig,
    snapshot: &nc::Snapshot<T>,
    matcher: &mut nc::Matcher,
    render: &R,
    width: u16,
    clear_mode: ClearMode,
    above: &[usize],
    below: &[usize],
    mut item_iter: impl Iterator<Item = (nc::Item<'a, T>, bool)>,
    chars: &PickerChars,
) -> io::Result<()> {
    // render above the selection
    for (item_height, (item, queued)) in above.iter().rev().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Tail, _, false>(
            writer,
            buffer,
            width,
            clear_mode,
            config,
            item,
            queued,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
            chars,
        )?;
    }

    // render the selection
    let (item, queued) = item_iter.next().unwrap();
    draw_single_match::<_, _, Head, _, true>(
        writer,
        buffer,
        width,
        clear_mode,
        config,
        item,
        queued,
        snapshot,
        matcher,
        as_u16(below[0]),
        render,
        chars,
    )?;

    // render below the selection
    for (item_height, (item, queued)) in below[1..].iter().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Head, _, false>(
            writer,
            buffer,
            width,
            clear_mode,
            config,
            item,
            queued,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
            chars,
        )?;
    }

    Ok(())
}

fn decimal_width(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
}

#[allow(clippy::too_many_arguments)]
fn draw_match_counts<W: io::Write + ?Sized>(
    writer: &mut W,
    width: u16,
    clear_mode: ClearMode,
    matched: u32,
    total: u32,
    multi: Option<(u32, Option<NonZero<u32>>)>,
    status_marker: Option<char>,
    chars: &PickerChars,
) -> io::Result<()> {
    if clear_mode != ClearMode::All {
        writer
            .queue(ResetColor)?
            .queue(SetAttribute(Attribute::Reset))?;
        if clear_mode == ClearMode::Line {
            writer.queue(Clear(ClearType::CurrentLine))?;
        }
    }

    let mut occupied = status_marker.unwrap_or(' ').width().unwrap_or(0)
        + 1
        + decimal_width(matched)
        + 1
        + decimal_width(total);
    if let Some((ct, op)) = multi {
        occupied += 2 + decimal_width(ct) + 1;
        if let Some(max) = op {
            occupied += 1 + decimal_width(max.get());
        }
    }

    if occupied > usize::from(width) {
        if clear_mode == ClearMode::Exact {
            write_spaces(writer, usize::from(width))?;
        }
        return Ok(());
    }

    writer
        .queue(SetAttribute(Attribute::Italic))?
        .queue(SetForegroundColor(Color::Green))?
        .queue(Print(status_marker.unwrap_or(' ')))?
        .queue(Print(" "))?
        .queue(Print(matched))?
        .queue(Print("/"))?
        .queue(Print(total))?;
    if let Some((ct, op)) = multi {
        writer
            .queue(SetForegroundColor(Color::Grey))?
            .queue(Print(" ("))?
            .queue(Print(ct))?;
        if let Some(max) = op {
            writer.queue(Print("/"))?.queue(Print(max))?;
        }
        writer.queue(Print(")"))?;
    }

    let fill_width = usize::from(width).saturating_sub(occupied);
    if fill_width != 0 {
        writer
            .queue(SetForegroundColor(Color::Grey))?
            .queue(Print(" "))?;
        for _ in 1..fill_width {
            writer.queue(Print(chars.separator))?;
        }
    }

    writer
        .queue(ResetColor)?
        .queue(SetAttribute(Attribute::Reset))?;

    Ok(())
}

fn draw_whitespace<W: Write + ?Sized>(
    writer: &mut W,
    width: u16,
    clear_mode: ClearMode,
    mut height: u16,
) -> io::Result<()> {
    while height > 0 {
        height -= 1;
        if clear_mode == ClearMode::Line {
            writer.queue(Clear(ClearType::CurrentLine))?;
        } else if clear_mode == ClearMode::Exact {
            write_spaces(writer, usize::from(width))?;
        }
        writer.queue(MoveToNextLine(1))?;
    }

    Ok(())
}

impl<T: Send + Sync + 'static, R: Render<T>> MatchList<T, R> {
    pub fn draw_status<W: Write + ?Sized>(
        &self,
        width: u16,
        clear_mode: ClearMode,
        writer: &mut W,
        multi: Option<(u32, Option<NonZero<u32>>)>,
        status_marker: Option<char>,
        chars: &PickerChars,
    ) -> std::io::Result<()> {
        let snapshot = self.nucleo.snapshot();
        draw_match_counts(
            writer,
            width,
            clear_mode,
            snapshot.matched_item_count(),
            snapshot.item_count(),
            multi,
            status_marker,
            chars,
        )
    }

    pub fn draw_items<W: Write + ?Sized, F: FnMut(u32) -> bool>(
        &mut self,
        width: u16,
        clear_mode: ClearMode,
        writer: &mut W,
        chars: &PickerChars,
        mut is_queued: F,
    ) -> std::io::Result<()> {
        let snapshot = self.nucleo.snapshot();
        let matched_item_count = snapshot.matched_item_count();
        let total_whitespace = self.whitespace();

        // draw the matches
        if self.config.reversed {
            if matched_item_count != 0 {
                let items = snapshot.matches()[self.selection_range()]
                    .iter()
                    .map(|&m| unsafe { (snapshot.get_item_unchecked(m.idx), is_queued(m.idx)) });
                draw_matches(
                    writer,
                    &mut self.scratch,
                    &self.config,
                    snapshot,
                    &mut self.matcher,
                    self.render.as_ref(),
                    width,
                    clear_mode,
                    &self.above,
                    &self.below,
                    items,
                    chars,
                )?;
            }

            draw_whitespace(writer, width, clear_mode, total_whitespace)?;
        } else {
            // skip / clear whitespace if necessary
            draw_whitespace(writer, width, clear_mode, total_whitespace)?;

            if matched_item_count != 0 {
                let items = snapshot.matches()[self.selection_range()]
                    .iter()
                    .map(|&m| unsafe { (snapshot.get_item_unchecked(m.idx), is_queued(m.idx)) });
                draw_matches(
                    writer,
                    &mut self.scratch,
                    &self.config,
                    snapshot,
                    &mut self.matcher,
                    self.render.as_ref(),
                    width,
                    clear_mode,
                    &self.above,
                    &self.below,
                    items.rev(),
                    chars,
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{PickerChars, frame::ClearMode};

    use super::{decimal_width, draw_match_counts};

    fn rendered_prefix(width: u16, status_marker: Option<char>) -> String {
        let mut output = Vec::new();
        draw_match_counts(
            &mut output,
            width,
            ClearMode::Line,
            3,
            5,
            None,
            status_marker,
            &PickerChars::new(),
        )
        .unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn status_line() {
        assert!(rendered_prefix(12, None).contains("  3/5"));
        assert!(rendered_prefix(12, Some('⠏')).contains("⠏ 3/5"));
        assert!(rendered_prefix(12, Some('≈')).contains("≈ 3/5"));

        assert!(rendered_prefix(12, None).contains("─────"));

        assert!(!rendered_prefix(4, None).contains("3/5"));

        let exact = rendered_prefix(5, None);
        assert!(exact.contains("3/5"));
        assert!(!exact.contains("\x1b[K"));

        assert_eq!(decimal_width(0), 1);
        assert_eq!(decimal_width(9), 1);
        assert_eq!(decimal_width(10), 2);
        assert_eq!(decimal_width(u32::MAX), 10);
    }

    #[test]
    fn exact_status_clear_fills_a_too_narrow_line() {
        let mut output = Vec::new();
        draw_match_counts(
            &mut output,
            4,
            ClearMode::Exact,
            3,
            5,
            None,
            None,
            &PickerChars::new(),
        )
        .unwrap();

        assert!(output.ends_with(b"    "));
        assert!(!String::from_utf8(output).unwrap().contains("\x1b[K"));
    }
}
