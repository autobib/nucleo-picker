use std::ops::Range;

use nucleo as nc;

use super::{
    MatchListConfig, MatchListState,
    item::RenderedItem,
    span::{Head, KeepLines, Spanned, Tail},
    unicode::Span,
};
use crate::util::unicode::{AsciiProcessor, UnicodeProcessor};
use crate::{PickerChars, Render, draw::Surface, match_engine::MatchEngine, util::as_u16};

/// Reusable scratch space for match list rendering.
pub(super) struct RenderScratch {
    /// Spans used to render items.
    spans: Vec<Span>,
    /// Sub-slices of `spans` corresponding to lines.
    lines: Vec<Range<usize>>,
    /// Indices generated from a match.
    indices: Vec<u32>,
    matcher: nc::Matcher,
}

impl RenderScratch {
    /// Create a new buffer.
    pub fn new(config: nc::Config) -> Self {
        Self {
            spans: Vec::with_capacity(16),
            lines: Vec::with_capacity(4),
            indices: Vec::with_capacity(16),
            matcher: nc::Matcher::new(config),
        }
    }
}

/// The inner `match draw` implementation.
#[inline]
#[allow(clippy::too_many_arguments)]
fn draw_single_match<
    T: Send + Sync + 'static,
    R: Render<T>,
    L: KeepLines,
    S: Surface,
    const HOVERED: bool,
>(
    surface: &mut S,
    buffer: &mut RenderScratch,
    config: &MatchListConfig,
    item: nc::Item<'_, T>,
    queued: bool,
    snapshot: &nc::Snapshot<T>,
    height: u16,
    render: &R,
    chars: &PickerChars,
) -> Result<(), S::Error> {
    // generate the indices
    if config.highlight {
        buffer.indices.clear();
        snapshot.pattern().column_pattern(0).indices(
            item.matcher_columns[0].slice(..),
            &mut buffer.matcher,
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
            surface,
            HOVERED,
            queued,
            config.highlight_padding,
            config.highlight_line,
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
            surface,
            HOVERED,
            queued,
            config.highlight_padding,
            config.highlight_line,
            chars,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_matches<'a, T: Send + Sync + 'static, R: Render<T>, S: Surface>(
    surface: &mut S,
    buffer: &mut RenderScratch,
    config: &MatchListConfig,
    snapshot: &nc::Snapshot<T>,
    render: &R,
    above: &[usize],
    below: &[usize],
    mut item_iter: impl Iterator<Item = (nc::Item<'a, T>, bool)>,
    chars: &PickerChars,
) -> Result<(), S::Error> {
    // render above the hovered item
    for (item_height, (item, queued)) in above.iter().rev().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Tail, _, false>(
            surface,
            buffer,
            config,
            item,
            queued,
            snapshot,
            as_u16(*item_height),
            render,
            chars,
        )?;
    }

    // render the hovered item
    // SAFETY: both callers supply above.len() + below.len() matches, with the hovered item in below.
    // The loop above consumes only above.len() items.
    let (item, queued) = unsafe { item_iter.next().unwrap_unchecked() };
    draw_single_match::<_, _, Head, _, true>(
        surface,
        buffer,
        config,
        item,
        queued,
        snapshot,
        as_u16(below[0]),
        render,
        chars,
    )?;

    // render below the hovered item
    for (item_height, (item, queued)) in below[1..].iter().zip(item_iter.by_ref()) {
        draw_single_match::<_, _, Head, _, false>(
            surface,
            buffer,
            config,
            item,
            queued,
            snapshot,
            as_u16(*item_height),
            render,
            chars,
        )?;
    }

    Ok(())
}

fn draw_whitespace<S: Surface>(surface: &mut S, height: u16) -> Result<(), S::Error> {
    for _ in 0..height {
        surface.clear_line()?;
        surface.next_line()?;
    }
    Ok(())
}

impl MatchListState {
    pub fn draw_items<T: Send + Sync + 'static, R: Render<T>, S: Surface, F: FnMut(u32) -> bool>(
        &mut self,
        engine: &MatchEngine<T, R>,
        surface: &mut S,
        chars: &PickerChars,
        mut is_queued: F,
    ) -> Result<(), S::Error> {
        if surface.is_empty() {
            return Ok(());
        }
        let snapshot = engine.snapshot();
        let matched_item_count = snapshot.matched_item_count();
        let total_whitespace = self.layout.whitespace();

        if matched_item_count == 0 {
            return surface.clear();
        }

        let items = snapshot.matches()[self.layout.visible_range(&self.config)]
            .iter()
            .map(|&m| unsafe { (snapshot.get_item_unchecked(m.idx), is_queued(m.idx)) });
        if self.config.reversed {
            draw_matches(
                surface,
                &mut self.scratch,
                &self.config,
                snapshot,
                engine.renderer(),
                &self.layout.above,
                &self.layout.below,
                items,
                chars,
            )?;
            draw_whitespace(surface, total_whitespace)?;
        } else {
            draw_whitespace(surface, total_whitespace)?;
            draw_matches(
                surface,
                &mut self.scratch,
                &self.config,
                snapshot,
                engine.renderer(),
                &self.layout.above,
                &self.layout.below,
                items.rev(),
                chars,
            )?;
        }
        Ok(())
    }
}
