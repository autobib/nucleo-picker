use std::io::{self, Write};

use nucleo as nc;

use super::{
    item::RenderedItem,
    span::{Head, KeepLines, Spanned, Tail},
    unicode::{AsciiProcessor, UnicodeProcessor},
    IndexBuffer, MatchList, MatchListConfig, MatchListEvent,
};
use crate::{
    component::Component,
    util::{as_u16, as_u32},
    Render,
};

use crossterm::{
    cursor::{MoveToColumn, MoveToNextLine, MoveToPreviousLine},
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
    QueueableCommand,
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
    max_draw_length: u16, // the width not including the space for the selection marker
    config: &MatchListConfig,
    item: &nc::Item<'_, T>,
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

    match RenderedItem::new(item, render) {
        RenderedItem::Ascii(s) => Spanned::<'_, AsciiProcessor>::new(
            &buffer.indices,
            s,
            &mut buffer.spans,
            &mut buffer.lines,
            L::from_offset(height),
        )
        .queue_print(writer, SELECTED, max_draw_length, config.highlight_padding),
        RenderedItem::Unicode(r) => Spanned::<'_, UnicodeProcessor>::new(
            &buffer.indices,
            r.as_ref(),
            &mut buffer.spans,
            &mut buffer.lines,
            L::from_offset(height),
        )
        .queue_print(writer, SELECTED, max_draw_length, config.highlight_padding),
    }
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

impl<T: Send + Sync + 'static, R: Render<T>> Component for MatchList<T, R> {
    type Event = MatchListEvent;

    type Status = bool;

    fn handle(&mut self, e: Self::Event) -> bool {
        match e {
            MatchListEvent::Up(incr) => self.selection_incr(as_u32(incr)),
            MatchListEvent::Down(decr) => self.selection_decr(as_u32(decr)),
            MatchListEvent::Reset => self.reset(),
        }
    }

    fn draw<W: Write + ?Sized>(
        &mut self,
        width: u16,
        height: u16,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let match_list_height = height - 1;
        let match_list_width = width.saturating_sub(3);

        if match_list_height != self.size {
            self.resize(match_list_height);
        }

        let snapshot = self.nucleo.snapshot();

        // draw the matches
        let matched_item_count = snapshot.matched_item_count();

        if height == 1 {
            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
        } else if matched_item_count == 0 {
            writer.queue(MoveToNextLine(height - 1))?;
            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
            writer
                .queue(MoveToPreviousLine(1))?
                .queue(MoveToColumn(width - 1))?
                .queue(Clear(ClearType::FromCursorUp))?;
        } else {
            let total_whitespace = self.whitespace();

            // skip / clear whitespace if necessary
            if total_whitespace != 0 {
                writer
                    .queue(MoveToNextLine(self.whitespace()))?
                    .queue(MoveToColumn(width - 1))?
                    .queue(Clear(ClearType::FromCursorUp))?
                    .queue(MoveToColumn(0))?;
            }

            let mut item_iter = snapshot.matched_items(self.selection_range()).rev();

            // render above the selection
            for item_height in self.above.iter().rev() {
                let next_item = item_iter.next().unwrap();
                draw_single_match::<T, R, Tail, W, false>(
                    writer,
                    &mut self.scratch,
                    match_list_width,
                    &self.config,
                    &next_item,
                    snapshot,
                    &mut self.matcher,
                    as_u16(*item_height),
                    &self.render,
                )?;
            }

            // render the selection
            draw_single_match::<T, R, Head, W, true>(
                writer,
                &mut self.scratch,
                match_list_width,
                &self.config,
                &item_iter.next().unwrap(),
                snapshot,
                &mut self.matcher,
                as_u16(self.below[0]),
                &self.render,
            )?;

            // render below the selection
            for item_height in self.below[1..].iter() {
                draw_single_match::<T, R, Head, W, false>(
                    writer,
                    &mut self.scratch,
                    match_list_width,
                    &self.config,
                    &item_iter.next().unwrap(),
                    snapshot,
                    &mut self.matcher,
                    as_u16(*item_height),
                    &self.render,
                )?;
            }

            draw_match_counts(writer, matched_item_count, snapshot.item_count())?;
        }

        Ok(())
    }
}
