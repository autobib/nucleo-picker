#[cfg(test)]
mod tests;

use std::{io, marker::PhantomData, ops::Range, slice::Iter};

use crossterm::style::{Attribute, Color, ContentStyle, Stylize};

use super::unicode::{Span, spans_from_indices};
use crate::util::{line::print_line, unicode::Processor};
use crate::{PickerChars, rect::Rect};

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

#[cfg(test)]
struct All;

#[cfg(test)]
impl KeepLines for All {
    fn subslice<'a>(&self, lines: &'a [Range<usize>]) -> &'a [Range<usize>] {
        lines
    }

    fn from_offset(_: u16) -> Self {
        Self
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

    /// Compute the maximum number of bytes over all lines.
    #[inline]
    fn max_line_bytes(&self) -> usize {
        let mut max_line_bytes = 0;
        for line in self.lines() {
            if !line.is_empty() {
                max_line_bytes = max_line_bytes
                    .max(line.last().unwrap().range.end - line.first().unwrap().range.start);
            }
        }

        max_line_bytes
    }

    /// Return the viewport origin in source columns, accounting for highlight padding
    /// and keeping the first highlight beyond the leading ellipsis when scrolled.
    #[inline]
    fn required_offset(&self, max_width: u16, highlight_padding: u16) -> usize {
        if max_width == 0 {
            return 0;
        }
        let mut required_width = 0;
        let mut first_column = usize::MAX;
        for line in self.lines() {
            let Some(first) = line.iter().find(|span| span.is_match) else {
                continue;
            };
            let last = line.iter().rfind(|span| span.is_match).unwrap();
            let before = P::width(&self.rendered[line[0].range.start..first.range.start]);
            first_column = first_column.min(before);
            if first_column <= 1 {
                return 0;
            }

            let limit = first_column
                .saturating_sub(1)
                .saturating_add(usize::from(max_width))
                .saturating_sub(usize::from(highlight_padding));
            let matched = P::width_up_to(
                &self.rendered[first.range.start..last.range.end],
                limit.saturating_sub(before),
            );
            required_width = required_width.max(before + matched);
        }
        let desired = required_width
            .saturating_add(usize::from(highlight_padding))
            .saturating_sub(usize::from(max_width));
        desired.min(first_column.saturating_sub(1))
    }

    /// Print the header for each line, which is either two spaces or styled indicator. This also
    /// sets the highlighting features for the given line.
    #[inline]
    fn start_line<D: Rect>(
        rect: &mut D,
        selected: bool,
        queued: bool,
        prefix_width: u16,
        highlight_line: bool,
        chars: &PickerChars,
    ) -> io::Result<()> {
        if prefix_width == 0 {
            return Ok(());
        }

        if selected {
            // print the line as bold, and with a 'selection' marker
            rect.set_attribute(Attribute::Bold)?;
            if !highlight_line {
                rect.set_background(Color::DarkGrey)?;
            }
            rect.print_styled(chars.selection.magenta())?;
        } else {
            // print a blank instead
            rect.print(" ")?;
        }

        if prefix_width >= 2 {
            if queued {
                rect.print_styled(chars.queued.magenta())?;
            } else {
                rect.print(" ")?;
            }
        }

        if selected && highlight_line {
            rect.set_background(Color::DarkGrey)?;
        }

        Ok(())
    }

    /// Printing a string slice to the given rectangle with highlighting.
    #[inline]
    fn print_span<D: Rect>(rect: &mut D, to_print: &str, highlight: bool) -> io::Result<()> {
        if highlight {
            rect.print_styled(to_print.cyan())?;
        } else {
            rect.print(to_print)?;
        }
        Ok(())
    }

    /// Clean up after printing the line by resetting any display styling and moving to the
    /// next line.
    #[inline]
    fn finish_line<D: Rect>(
        rect: &mut D,
        remaining: impl FnOnce() -> u16,
        selected: bool,
        fill_highlight: bool,
    ) -> io::Result<()> {
        if fill_highlight {
            rect.spaces(remaining())?;
        }
        if selected {
            rect.set_attribute(Attribute::Reset)?;
        }
        rect.next_line()
    }

    /// Print into the rectangle with styling to match if the item is selected or not.
    #[inline]
    pub fn queue_print<D: Rect>(
        &self,
        rect: &mut D,
        selected: bool,
        queued: bool,
        highlight_padding: u16,
        highlight_line: bool,
        chars: &PickerChars,
    ) -> io::Result<()> {
        let width = rect.width().get();
        let prefix_width = width.min(2);
        let max_width = width.saturating_sub(prefix_width);
        let fill_highlight = selected && highlight_line;

        if self.max_line_bytes() <= max_width.saturating_sub(highlight_padding) as usize {
            // Fast path: all of the lines are short, so we can just render them without any unicode width
            // checks. This should be the case for the majority of situations, unless the screen is
            // very narrow or the rendered items are very wide.
            //
            // This check is safe since the only unicode characters which require two columns consist of
            // at least two bytes, so the number of bytes is always an upper bound for the number of
            // columns.
            //
            // If the input is ASCII, this check is optimal.
            for line in self.lines() {
                if !fill_highlight {
                    rect.clear_line()?;
                }
                Self::start_line(rect, selected, queued, prefix_width, highlight_line, chars)?;
                for span in line {
                    Self::print_span(rect, self.index_in(span), span.is_match)?;
                }

                let remaining_capacity = || {
                    let printed_width = if line.is_empty() {
                        0
                    } else {
                        P::width(
                            &self.rendered[line[0].range.start..line.last().unwrap().range.end],
                        )
                    };
                    max_width.saturating_sub(printed_width as u16)
                };
                Self::finish_line(rect, remaining_capacity, selected, fill_highlight)?;
            }
        } else {
            let offset = self.required_offset(max_width, highlight_padding);

            for line in self.lines() {
                if !fill_highlight {
                    rect.clear_line()?;
                }
                Self::start_line(rect, selected, queued, prefix_width, highlight_line, chars)?;
                let remaining_capacity =
                    self.queue_print_line(rect, line, offset, max_width, chars)?;
                Self::finish_line(rect, || remaining_capacity, selected, fill_highlight)?;
            }
        }
        Ok(())
    }

    /// Print a single line (represented as a slice of [`Span`]) to the terminal screen, with the
    /// given `offset` and the width of the screen in columns, as `capacity`.
    #[inline]
    fn queue_print_line<D: Rect>(
        &self,
        rect: &mut D,
        line: &[Span],
        offset: usize,
        capacity: u16,
        chars: &PickerChars,
    ) -> io::Result<u16> {
        let text = match (line.first(), line.last()) {
            (Some(first), Some(last)) => &self.rendered[first.range.start..last.range.end],
            _ => "",
        };
        let spans = line.iter().map(|span| {
            let style = if span.is_match {
                ContentStyle::new().cyan()
            } else {
                ContentStyle::new()
            };
            style.apply(self.index_in(span))
        });
        print_line::<P, _>(rect, text, spans, offset, capacity, chars.ellipsis)
    }

    /// Compute the string slice corresponding to the given [`Span`].
    ///
    /// # Panics
    /// This method must be called with a span with `range.start` and `range.end` corresponding to
    /// valid unicode indices in `rendered`.
    #[inline]
    fn index_in(&self, span: &Span) -> &str {
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
