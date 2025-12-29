use std::io::{self, Write};

use nucleo as nc;

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
    item: &(nc::Item<'_, T>, bool),
    snapshot: &nc::Snapshot<T>,
    matcher: &mut nc::Matcher,
    height: u16,
    render: &R,
) -> io::Result<()> {
    let (item, queued) = item;
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

    match RenderedItem::new(item, render) {
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
            *queued,
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
            *queued,
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
    for (item_height, item) in above.iter().rev().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Tail, _, false>(
            writer,
            buffer,
            match_list_width,
            config,
            &item,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
        )?;
    }

    // render the selection
    draw_single_match::<_, _, Head, _, true>(
        writer,
        buffer,
        match_list_width,
        config,
        &item_iter.next().unwrap(),
        snapshot,
        matcher,
        as_u16(below[0]),
        render,
    )?;

    // render below the selection
    for (item_height, item) in below[1..].iter().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Head, _, false>(
            writer,
            buffer,
            match_list_width,
            config,
            &item,
            snapshot,
            matcher,
            as_u16(*item_height),
            render,
        )?;
    }

    Ok(())
}

fn draw_match_counts<W: io::Write + ?Sized>(
    writer: &mut W,
    matched: u32,
    total: u32,
) -> io::Result<()> {
    writer
        .queue(SetAttribute(Attribute::Italic))?
        .queue(SetForegroundColor(Color::Green))?
        .queue(Print("  "))?
        .queue(Print(matched))?
        .queue(Print("/"))?
        .queue(Print(total))?
        .queue(SetAttribute(Attribute::Reset))?
        .queue(ResetColor)?
        .queue(Clear(ClearType::UntilNewLine))?;

    Ok(())
}

impl<T: Send + Sync + 'static, R: Render<T>> MatchList<T, R> {
    pub fn draw<W: Write + ?Sized, F: FnMut(u32) -> bool>(
        &mut self,
        width: u16,
        height: u16,
        writer: &mut W,
        mut is_queued: F,
    ) -> std::io::Result<()> {
        let match_list_height = height - 1;
        let match_list_width = width.saturating_sub(3);

        if match_list_height != self.size {
            self.resize(match_list_height);
        }

        let snapshot = self.nucleo.snapshot();
        let matched_item_count = snapshot.matched_item_count();

        if height == 1 {
            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
            return Ok(());
        }

        let mut total_whitespace = self.whitespace();

        // draw the matches
        if self.config.reversed {
            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
            writer.queue(MoveToNextLine(1))?;

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

            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
        }

        Ok(())
    }
}
