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
use crate::{Render, util::as_u16};

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
    max_draw_length: u16, // the width for the line itself (i.e.
    // not including the space for the selection marker)
    config: &MatchListConfig,
    item: nc::Item<'_, T>,
    queued: bool,
    snapshot: &nc::Snapshot<T>,
    matcher: &mut nc::Matcher,
    height: u16,
    render: &R,
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
            max_draw_length,
            config.highlight_padding,
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
            max_draw_length,
            config.highlight_padding,
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
    match_list_width: u16,
    above: &[usize],
    below: &[usize],
    mut item_iter: impl Iterator<Item = (nc::Item<'a, T>, bool)>,
) -> io::Result<()> {
    // render above the selection
    for (item_height, (item, queued)) in above.iter().rev().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Tail, _, false>(
            writer,
            buffer,
            match_list_width,
            config,
            item,
            queued,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
        )?;
    }

    // render the selection
    let (item, queued) = item_iter.next().unwrap();
    draw_single_match::<_, _, Head, _, true>(
        writer,
        buffer,
        match_list_width,
        config,
        item,
        queued,
        snapshot,
        matcher,
        as_u16(below[0]),
        render,
    )?;

    // render below the selection
    for (item_height, (item, queued)) in below[1..].iter().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Head, _, false>(
            writer,
            buffer,
            match_list_width,
            config,
            item,
            queued,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
        )?;
    }

    Ok(())
}

fn decimal_width(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
}

fn draw_match_counts<W: io::Write + ?Sized>(
    writer: &mut W,
    width: u16,
    matched: u32,
    total: u32,
    multi: Option<(u32, Option<NonZero<u32>>)>,
    status_marker: Option<char>,
) -> io::Result<()> {
    let mut occupied = status_marker.unwrap_or(' ').width().unwrap_or(0)
        + 1
        + decimal_width(matched)
        + 1
        + decimal_width(total);
    writer
        .queue(SetAttribute(Attribute::Italic))?
        .queue(SetForegroundColor(Color::Green))?
        .queue(Print(status_marker.unwrap_or(' ')))?
        .queue(Print(" "))?
        .queue(Print(matched))?
        .queue(Print("/"))?
        .queue(Print(total))?;
    if let Some((ct, op)) = multi {
        occupied += 2 + decimal_width(ct) + 1;
        writer
            .queue(SetForegroundColor(Color::Grey))?
            .queue(Print(" ("))?
            .queue(Print(ct))?;
        if let Some(max) = op {
            occupied += 1 + decimal_width(max.get());
            writer.queue(Print("/"))?.queue(Print(max))?;
        }
        writer.queue(Print(")"))?;
    }

    let fill_width = usize::from(width).saturating_sub(occupied);
    if fill_width != 0 {
        writer
            .queue(SetForegroundColor(Color::Grey))?
            .queue(Print(" "))?
            .queue(Print("─".repeat(fill_width - 1)))?;
    }

    writer
        .queue(ResetColor)?
        .queue(SetAttribute(Attribute::Reset))?
        .queue(Clear(ClearType::UntilNewLine))?;

    Ok(())
}

impl<T: Send + Sync + 'static, R: Render<T>> MatchList<T, R> {
    pub fn draw_status<W: Write + ?Sized>(
        &self,
        width: u16,
        writer: &mut W,
        multi: Option<(u32, Option<NonZero<u32>>)>,
        status_marker: Option<char>,
    ) -> std::io::Result<()> {
        let snapshot = self.nucleo.snapshot();
        draw_match_counts(
            writer,
            width,
            snapshot.matched_item_count(),
            snapshot.item_count(),
            multi,
            status_marker,
        )
    }

    pub fn draw_items<W: Write + ?Sized, F: FnMut(u32) -> bool>(
        &mut self,
        width: u16,
        writer: &mut W,
        mut is_queued: F,
    ) -> std::io::Result<()> {
        let match_list_width = width.saturating_sub(3);
        let snapshot = self.nucleo.snapshot();
        let matched_item_count = snapshot.matched_item_count();
        let mut total_whitespace = self.whitespace();

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
                    match_list_width,
                    &self.above,
                    &self.below,
                    items,
                )?;
            }

            if total_whitespace > 0 {
                writer.queue(Clear(ClearType::FromCursorDown))?;
            }
        } else {
            // skip / clear whitespace if necessary
            while total_whitespace > 0 {
                total_whitespace -= 1;
                writer
                    .queue(Clear(ClearType::UntilNewLine))?
                    .queue(MoveToNextLine(1))?;
            }

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
                    match_list_width,
                    &self.above,
                    &self.below,
                    items.rev(),
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{decimal_width, draw_match_counts};

    fn rendered_prefix(status_marker: Option<char>) -> String {
        let mut output = Vec::new();
        draw_match_counts(&mut output, 12, 3, 5, None, status_marker).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn status_markers_precede_match_counts() {
        assert!(rendered_prefix(None).contains("  3/5"));
        assert!(rendered_prefix(Some('⠏')).contains("⠏ 3/5"));
        assert!(rendered_prefix(Some('≈')).contains("≈ 3/5"));
    }

    #[test]
    fn status_line_fills_remaining_width() {
        assert!(rendered_prefix(None).contains("─────"));
    }

    #[test]
    fn decimal_width_handles_zero_and_powers_of_ten() {
        assert_eq!(decimal_width(0), 1);
        assert_eq!(decimal_width(9), 1);
        assert_eq!(decimal_width(10), 2);
        assert_eq!(decimal_width(u32::MAX), 10);
    }
}
