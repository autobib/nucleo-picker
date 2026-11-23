#[cfg(test)]
mod tests;

use std::{marker::PhantomData, ops::Range, slice::Iter};

use super::unicode::{Span, spans_from_indices};
use crate::util::{line::print_line, unicode::Processor};
use crate::{
    PickerChars,
    draw::{Color, Style, StyledStr, Surface},
};

/// An iterator over lines, as span slices.
pub struct SpannedLines<'a> {
    iter: Iter<'a, Range<usize>>,
    spans: &'a [Span],
}

impl<'a> Iterator for SpannedLines<'a> {
    type Item = &'a [Span];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(rg) => Some(&self.spans[rg.start..rg.end]),
            None => None,
        }
    }
}

pub trait KeepLines {
    fn from_offset(offset: u16) -> Self;

    fn subslice<'a>(&self, lines: &'a [Range<usize>]) -> &'a [Range<usize>];
}

pub struct Tail(usize);

impl KeepLines for Tail {
    fn subslice<'a>(&self, lines: &'a [Range<usize>]) -> &'a [Range<usize>] {
        &lines[lines.len() - self.0..]
    }

    fn from_offset(offset: u16) -> Self {
        Self(offset as usize)
    }
}

pub struct Head(usize);

impl KeepLines for Head {
    fn subslice<'a>(&self, lines: &'a [Range<usize>]) -> &'a [Range<usize>] {
        &lines[..self.0]
    }

    fn from_offset(offset: u16) -> Self {
        Self(offset as usize)
    }
}

/// Represent additional data on top of a string slice.
///
/// The `spans` are guaranteed to not contain newlines. In order to determine which spans belong to
/// which line, `lines` consists of contiguous sub-slices of `spans`.
#[derive(Debug)]
pub struct Spanned<'a, P> {
    rendered: &'a str,
    spans: &'a [Span],
    lines: &'a [Range<usize>],
    _marker: PhantomData<P>,
}

impl<'a, P: Processor> Spanned<'a, P> {
    #[inline]
    pub fn new<L: KeepLines>(
        indices: &[u32],
        rendered: &'a str,
        spans: &'a mut Vec<Span>,
        lines: &'a mut Vec<Range<usize>>,
        keep_lines: L,
    ) -> Self {
        spans_from_indices::<P>(indices, rendered, spans, lines);
        Self {
            rendered,
            spans,
            lines: keep_lines.subslice(lines),
            _marker: PhantomData,
        }
    }

    /// Check whether all the lines fit.
    #[inline]
    fn lines_fit(&self, capacity: u16) -> bool {
        let limit = usize::from(capacity) + 1;
        self.lines().all(|line| {
            let text = self.line_text(line);
            text.len() < limit || P::width_up_to(text, limit) < limit
        })
    }

    /// Return the viewport origin in source columns, accounting for highlight padding
    /// and keeping the first highlight beyond the leading ellipsis when scrolled.
    #[inline]
    fn required_offset(&self, max_width: u16, highlight_padding: u16) -> usize {
        if max_width == 0 {
            return 0;
        }
        let width = usize::from(max_width);
        let mut offset = 0;
        let mut max_offset = usize::MAX;
        for line in self.lines() {
            let Some(first) = line.iter().find(|span| span.is_match) else {
                continue;
            };
            let last = line.iter().rfind(|span| span.is_match).unwrap();
            let before = P::width(&self.rendered[line[0].range.start..first.range.start]);
            max_offset = max_offset.min(before.saturating_sub(1));
            if max_offset == 0 {
                return 0;
            }

            let matched = P::width_up_to(&self.rendered[first.range.start..last.range.end], width);
            // A suffix needs a column for either its text or a trailing ellipsis.
            let trailing = P::width_up_to(
                &self.rendered[last.range.end..line.last().unwrap().range.end],
                1,
            );
            let required = (before + matched)
                .saturating_add(usize::from(highlight_padding).max(trailing))
                .saturating_sub(width);
            offset = offset.max(required).min(max_offset);
        }
        offset
    }

    /// Print the header for each line, which is either two spaces or styled indicator. This also
    /// sets the highlighting features for the given line.
    #[inline]
    fn start_line<S: Surface>(
        surface: &mut S,
        hovered: bool,
        queued: bool,
        prefix_width: u16,
        highlight_line: bool,
        chars: &PickerChars,
    ) -> Result<(), S::Error> {
        if prefix_width == 0 {
            return Ok(());
        }

        let mut style = Style::new();
        if hovered {
            // print the line as bold, and with a 'hovered-item' marker
            style = style.bold();
            if !highlight_line {
                style = style.bg(Color::DarkGrey);
            }
            surface.text(
                chars.hover.encode_utf8(&mut [0; 4]),
                style.fg(Color::Magenta),
            )?;
        } else {
            // print a blank instead
            surface.spaces(1, style)?;
        }

        if prefix_width >= 2 {
            if queued {
                surface.text(
                    chars.queued.encode_utf8(&mut [0; 4]),
                    style.fg(Color::Magenta),
                )?;
            } else {
                surface.spaces(1, style)?;
            }
        }

        Ok(())
    }

    /// Print a string slice to the given rectangle with highlighting.
    #[inline]
    fn styled_line<'s>(
        &'s self,
        line: &'s [Span],
        style: Style,
    ) -> impl Iterator<Item = StyledStr<'a>> + 's {
        line.iter().map(move |span| {
            let style = if span.is_match {
                style.fg(Color::Cyan)
            } else {
                style
            };
            style.apply(self.index_in(span))
        })
    }

    #[inline]
    fn line_text(&self, line: &[Span]) -> &'a str {
        match (line.first(), line.last()) {
            (Some(first), Some(last)) => &self.rendered[first.range.start..last.range.end],
            _ => "",
        }
    }

    /// Clean up after printing the line by printing any remaining whitespace (for instance for fill
    /// highlight) and moving to the next line.
    #[inline]
    fn finish_line<S: Surface>(
        surface: &mut S,
        row: &mut u16,
        remaining: u16,
        style: Style,
        fill_highlight: bool,
    ) -> Result<(), S::Error> {
        if fill_highlight {
            surface.spaces(remaining, style)?;
        }
        *row += 1;
        Ok(())
    }

    /// Print into the rectangle with styling to match if the item is hovered or not.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn queue_print<S: Surface>(
        &self,
        surface: &mut S,
        row: &mut u16,
        hovered: bool,
        queued: bool,
        highlight_padding: u16,
        highlight_line: bool,
        chars: &PickerChars,
    ) -> Result<(), S::Error> {
        if surface.is_empty() {
            return Ok(());
        }
        let width = surface.width();
        let prefix_width = width.min(2);
        let max_width = width.saturating_sub(prefix_width);
        let fill_highlight = hovered && highlight_line;
        let style = if hovered {
            Style::new().bold().bg(Color::DarkGrey)
        } else {
            Style::new()
        };

        if self.lines_fit(max_width.saturating_sub(highlight_padding)) {
            // Fast path: all of the lines are short, so we can just render them without any width
            // computations. This should also be the typical case so it is worthwhile to use this
            // optimization
            for line in self.lines() {
                surface.begin_row(*row, !fill_highlight)?;
                Self::start_line(
                    surface,
                    hovered,
                    queued,
                    prefix_width,
                    highlight_line,
                    chars,
                )?;
                surface.styled_text(self.line_text(line), self.styled_line(line, style))?;

                let remaining_capacity = if fill_highlight {
                    let printed_width = if line.is_empty() {
                        0
                    } else {
                        P::width(
                            &self.rendered[line[0].range.start..line.last().unwrap().range.end],
                        )
                    };
                    max_width.saturating_sub(printed_width as u16)
                } else {
                    0
                };
                Self::finish_line(surface, row, remaining_capacity, style, fill_highlight)?;
            }
        } else {
            let offset = self.required_offset(max_width, highlight_padding);

            for line in self.lines() {
                surface.begin_row(*row, !fill_highlight)?;
                Self::start_line(
                    surface,
                    hovered,
                    queued,
                    prefix_width,
                    highlight_line,
                    chars,
                )?;
                let remaining_capacity =
                    self.queue_print_line(surface, line, offset, max_width, chars, style)?;
                Self::finish_line(surface, row, remaining_capacity, style, fill_highlight)?;
            }
        }
        Ok(())
    }

    /// Print a single line (represented as a slice of [`Span`]) to the terminal screen, with the
    /// given `offset` and the width of the screen in columns, as `capacity`.
    #[inline]
    fn queue_print_line<S: Surface>(
        &self,
        surface: &mut S,
        line: &[Span],
        offset: usize,
        capacity: u16,
        chars: &PickerChars,
        style: Style,
    ) -> Result<u16, S::Error> {
        print_line::<P, _>(
            surface,
            self.line_text(line),
            self.styled_line(line, style),
            offset,
            capacity,
            chars.ellipsis,
            style,
        )
    }

    /// Compute the string slice corresponding to the given [`Span`].
    ///
    /// # Panics
    /// This method must be called with a span with `range.start` and `range.end` corresponding to
    /// valid Unicode indices in `rendered`.
    #[inline]
    fn index_in(&self, span: &Span) -> &'a str {
        &self.rendered[span.range.start..span.range.end]
    }

    #[inline]
    fn lines(&self) -> SpannedLines<'_> {
        SpannedLines {
            iter: self.lines.iter(),
            spans: self.spans,
        }
    }
}
