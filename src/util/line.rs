use std::{io, ops::Range};

use crossterm::style::StyledContent;

use super::unicode::Processor;
use crate::rect::Rect;

#[cfg(test)]
mod tests;

struct LineClip {
    visible: Range<usize>,
    leading: u16,
    trailing: u16,
    remaining: u16,
}

fn clip_line<P: Processor>(text: &str, offset: usize, capacity: u16) -> LineClip {
    let mut clip = LineClip {
        visible: 0..0,
        leading: 0,
        trailing: 0,
        remaining: capacity,
    };
    if capacity == 0 || text.is_empty() {
        return clip;
    }
    if offset != 0 {
        clip.leading = 1;
        clip.remaining -= 1;
        if clip.remaining == 0 {
            return clip;
        }
    }

    let skip_columns = offset.saturating_add(usize::from(clip.leading));
    let mut graphemes = P::grapheme_index_widths(text).peekable();
    let mut skipped = 0;
    while skipped < skip_columns {
        let Some((_, width)) = graphemes.next() else {
            clip.visible = text.len()..text.len();
            return clip;
        };
        skipped += width;
    }
    let alignment = (skipped - skip_columns).min(usize::from(clip.remaining)) as u16;
    clip.leading += alignment;
    clip.remaining -= alignment;
    if clip.remaining == 0 {
        return clip;
    }

    clip.visible.start = graphemes.peek().map_or(text.len(), |&(index, _)| index);
    let mut reserved = (clip.visible.start, clip.remaining);
    for (index, width) in graphemes {
        if width > usize::from(clip.remaining) {
            let (end, markers) = if clip.remaining == 0 {
                reserved
            } else {
                (index, clip.remaining)
            };
            clip.visible.end = end;
            clip.trailing = markers;
            clip.remaining = 0;
            return clip;
        }
        // Keep the boundary before the last occupied columns, including any following
        // zero-width graphemes, until we know whether the line fits exactly.
        if width != 0 && width == usize::from(clip.remaining) {
            reserved = (index, clip.remaining);
        }
        clip.remaining -= width as u16;
    }
    clip.visible.end = text.len();
    clip
}

/// Print a single line at the given offset with the provided capacity, returning the number of
/// unused columns.
///
/// The caller is required to position the cursor correctly. The span contents and `text` must be
/// identical.
///
/// Offset is the viewport origin in source columns. At nonzero offsets, the leading
/// ellipsis occupies the first viewport cell, so text starts at `offset + 1` (rounded forward to a
/// grapheme boundary).
pub fn print_line<'a, P: Processor, D: Rect>(
    rect: &mut D,
    text: &'a str,
    spans: impl Iterator<Item = StyledContent<&'a str>>,
    offset: usize,
    capacity: u16,
    ellipsis: char,
) -> io::Result<u16> {
    let clip = clip_line::<P>(text, offset, capacity);
    for _ in 0..clip.leading {
        rect.print(ellipsis)?;
    }
    if !clip.visible.is_empty() {
        let mut start = 0;
        for span in spans {
            let content = span.content();
            let end = start + content.len();
            if end > clip.visible.start {
                let from = clip.visible.start.saturating_sub(start);
                let to = content.len().min(clip.visible.end - start);
                rect.print_styled(span.style().apply(&content[from..to]))?;
                // Crossterm resets foreground, but not underline colour, for attribute-free spans.
                if span.style().underline_color.is_some() && span.style().attributes.is_empty() {
                    rect.reset_style()?;
                }
            }
            if end >= clip.visible.end {
                break;
            }
            start = end;
        }
    }
    for _ in 0..clip.trailing {
        rect.print(ellipsis)?;
    }
    Ok(clip.remaining)
}
