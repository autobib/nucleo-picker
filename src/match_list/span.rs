#[cfg(test)]
mod tests;

use std::{
    io::{self, Write},
    iter::once,
    marker::PhantomData,
    ops::Range,
    slice::Iter,
};

use crossterm::{
    cursor::{MoveToColumn, MoveToNextLine},
    style::{
        Attribute, Color, Print, PrintStyledContent, SetAttribute, SetBackgroundColor, Stylize,
    },
    terminal::{Clear, ClearType},
    QueueableCommand,
};

use super::unicode::{consume, spans_from_indices, truncate, Processor, Span};

const ELLIPSIS: char = '…';

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

struct All;

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

    /// Returns the width (possibly 0) required to render all of the spans which require highlighting.
    #[inline]
    fn required_width(&self) -> usize {
        let mut required_width = 0;

        for line in self.lines() {
            // find the 'rightmost' highlighted span
            if let Some(span) = line.iter().rev().find(|span| span.is_match) {
                required_width = required_width.max(
                    // spans[0] must exist since `find` returned something
                    P::width(&self.rendered[line[0].range.start..span.range.end]),
                );
            }
        }
        required_width
    }

    /// Returns the optiomal offset (in terminal columns) for printing the given line.
    /// The offset automatically reserves an extra space for a single indicator symbol (such as an
    /// ellipsis), if required. The ellipsis should be printed whenever the returned value is not
    /// `0`.
    #[inline]
    fn required_offset(&self, max_width: u16, highlight_padding: u16) -> usize {
        match (self.required_width() + highlight_padding as usize).checked_sub(max_width as usize) {
            None | Some(0) => 0,
            Some(mut offset) => {
                // ideally, we would like to offset by `offset`; but we prefer highlighting
                // matches which are earlier in the string. Therefore, reduce `offset` so that it
                // lies before the first highlighted character in each line.

                let mut is_sharp = false; // if the offset cannot be increased because of a
                                          // highlighted char early in the match

                for line in self.lines() {
                    // find the 'leftmost' highlighted span.
                    if let Some(span) = line.iter().find(|span| span.is_match) {
                        let no_highlight_width =
                            P::width(&self.rendered[line[0].range.start..span.range.start]);
                        if no_highlight_width <= offset {
                            offset = no_highlight_width;
                            is_sharp = true;
                        }
                    }
                }

                // if the offset is not sharp, reserve an extra space for the ellipsis symbol
                if !is_sharp {
                    offset += 1;
                };

                // if the offset is exactly 1, set it to 0 since we can just print the first
                // character instead of the ellipsis
                if offset == 1 {
                    0
                } else {
                    offset
                }
            }
        }
    }

    /// Print the header for each line, which is either two spaces or styled indicator. This also
    /// sets the highlighting features for the given line.
    #[inline]
    fn start_line<W: Write + ?Sized>(stderr: &mut W, selected: bool) -> io::Result<()> {
        if selected {
            // print the line as bold, and with a 'selection' marker
            stderr
                .queue(SetAttribute(Attribute::Bold))?
                .queue(SetBackgroundColor(Color::DarkGrey))?
                .queue(PrintStyledContent("▌ ".magenta()))?;
        } else {
            // print a blank instead
            stderr.queue(Print("  "))?;
        }
        Ok(())
    }

    /// Queue a string slice for printing to stderr, either highlighted or printed.
    #[inline]
    fn print_span<W: Write + ?Sized>(
        stderr: &mut W,
        to_print: &str,
        highlight: bool,
    ) -> io::Result<()> {
        if highlight {
            stderr.queue(PrintStyledContent(to_print.cyan()))?;
        } else {
            stderr.queue(Print(to_print))?;
        }
        Ok(())
    }

    /// Clean up after printing the line by resetting any display styling, clearing any trailing
    /// characters, and moving to the next line.
    #[inline]
    fn finish_line<W: Write + ?Sized>(stderr: &mut W) -> io::Result<()> {
        stderr
            .queue(SetAttribute(Attribute::Reset))?
            .queue(Clear(ClearType::UntilNewLine))?
            .queue(MoveToNextLine(1))?;
        Ok(())
    }

    /// Print for display into a terminal with width `max_width`, and with styling to match if the
    /// item is selected or not.
    #[inline]
    pub fn queue_print<W: Write + ?Sized>(
        &self,
        stderr: &mut W,
        selected: bool,
        max_width: u16,
        highlight_padding: u16,
    ) -> io::Result<()> {
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
                Self::start_line(stderr, selected)?;
                for span in line {
                    Self::print_span(stderr, self.index_in(span), span.is_match)?;
                }
                Self::finish_line(stderr)?;
            }
        } else {
            let offset = self.required_offset(max_width, highlight_padding);

            for line in self.lines() {
                Self::start_line(stderr, selected)?;
                self.queue_print_line(stderr, line, offset, max_width)?;
                Self::finish_line(stderr)?;
            }
        }
        Ok(())
    }

    /// Print a single line (represented as a slice of [`Span`]) to the terminal screen, with the
    /// given `offset` and the width of the screen in columns, as `capacity`.
    #[inline]
    fn queue_print_line<W: Write + ?Sized>(
        &self,
        stderr: &mut W,
        line: &[Span],
        offset: usize,
        capacity: u16,
    ) -> io::Result<()> {
        let mut remaining_capacity = capacity;

        // do not print ellipsis if line is empty or the screen is extremely narrow
        if line.is_empty() || remaining_capacity == 0 {
            return Ok(());
        };

        if offset > 0 {
            // we just checked that `capacity != 0`
            remaining_capacity -= 1;
            stderr.queue(Print(ELLIPSIS))?;
        };

        // consume as much of the first span as required to overtake the offset. since the width of
        // the offset is bounded above by the width of the first span, this is guaranteed to occur
        // within the first span
        let first_span = &line[0];
        let (init, alignment) = consume::<P>(self.index_in(first_span), offset);
        let new_first_span = Span {
            range: first_span.range.start + init..first_span.range.end,
            is_match: first_span.is_match,
        };

        // print the extra alignment characters
        match (remaining_capacity as usize).checked_sub(alignment) {
            Some(new) => {
                remaining_capacity = new as u16;
                for _ in 0..alignment {
                    stderr.queue(Print(ELLIPSIS))?;
                }
            }
            None => return Ok(()),
        }

        // print as many spans as possible
        for span in once(&new_first_span).chain(line[1..].iter()) {
            let substr = self.index_in(span);
            match truncate::<P>(substr, remaining_capacity) {
                Ok(new) => {
                    remaining_capacity = new;
                    Self::print_span(stderr, substr, span.is_match)?;
                }
                Err((prefix, alignment)) => {
                    Self::print_span(stderr, prefix, span.is_match)?;
                    if alignment > 0 {
                        // there is already extra space; fill it
                        for _ in 0..alignment {
                            stderr.queue(Print(ELLIPSIS))?;
                        }
                    } else {
                        // overwrite the previous grapheme
                        let undo_width = P::last_grapheme_width(
                            &self.rendered[..span.range.start + prefix.len()],
                        );

                        stderr.queue(MoveToColumn(2 + capacity - undo_width as u16))?;
                        for _ in 0..undo_width {
                            stderr.queue(Print(ELLIPSIS))?;
                        }
                    }
                    return Ok(());
                }
            }
        }

        Ok(())
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
