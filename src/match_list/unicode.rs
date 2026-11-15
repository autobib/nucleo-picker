//! Utilities for handling Unicode display in the terminal.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::module_name_repetitions)]

use std::ops::Range;

use memchr::memchr_iter;

use crate::util::unicode::Processor;

/// A span corresponding to an unowned sub-slice of a string.
#[derive(Debug, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub is_match: bool,
}

/// Compute `spans` and `lines` corresponding to the provided indices in the given buffers.
///
/// Note that this will automatically clear the buffers.
///
/// The `spans` are guaranteed to not contain newlines. In order to determine which spans belong to
/// which line, `lines` consists of contiguous sub-slices of `spans`.
#[inline]
pub fn spans_from_indices<P: Processor>(
    indices: &[u32],
    rendered: &str,
    spans: &mut Vec<Span>,
    lines: &mut Vec<Range<usize>>,
) {
    spans.clear();
    lines.clear();

    let mut grapheme_index_iter = P::grapheme_index_widths(rendered);

    let mut iter_step_count = 0; // how many graphemes we have consumed
    let mut start = 0; // the current offset position for the next block
    let mut line_start = 0;
    let mut line_end = 0;

    for (left, right) in IndexSpans::new(indices) {
        let (middle, _) = grapheme_index_iter
            .nth(left - iter_step_count)
            .expect("Match index does not correspond to grapheme!");
        let end = match grapheme_index_iter.nth(right - left) {
            Some((end, _)) => {
                // + 2, since `nth` is zero-indexed and we called it twice
                iter_step_count = right + 2;
                end
            }
            _ => rendered.len(),
        };

        insert_unmatched_spans(
            spans,
            rendered,
            start,
            middle,
            lines,
            &mut line_start,
            &mut line_end,
        );

        // insert the highlighted span
        if middle != end {
            line_end += 1;
            spans.push(Span {
                range: middle..end,
                is_match: true,
            });
        }

        start = end;
    }

    insert_unmatched_spans(
        spans,
        rendered,
        start,
        rendered.len(),
        lines,
        &mut line_start,
        &mut line_end,
    );

    // insert the final line
    lines.push(line_start..line_end);
}

#[inline]
fn insert_unmatched_spans(
    spans: &mut Vec<Span>,
    rendered: &str,
    start: usize,
    middle: usize,
    lines: &mut Vec<Range<usize>>,
    line_start: &mut usize,
    line_end: &mut usize,
) {
    let mut span_start = start; // the byte offset of the current span
    let block = &rendered[start..middle];

    // iterate over possible newlines in the "non-match" block
    for linebreak_offset in memchr_iter(b'\n', block.as_bytes()) {
        let span_end = start + linebreak_offset;

        // insert the span if it is not empty after removing a possible trailing '\r'
        let range = if block[..linebreak_offset].ends_with('\r') {
            span_start..span_end - 1
        } else {
            span_start..span_end
        };
        if !range.is_empty() {
            *line_end += 1;
            spans.push(Span {
                range,
                is_match: false,
            });
        }
        lines.push(*line_start..*line_end);
        *line_start = *line_end;

        // exclude newline
        span_start = span_end + 1;
    }

    // insert any trailing characters
    if span_start != middle {
        *line_end += 1;
        spans.push(Span {
            range: span_start..middle,
            is_match: false,
        });
    }
}

struct IndexSpans<'a> {
    indices: &'a [u32],
    cursor: usize,
}

impl<'a> IndexSpans<'a> {
    fn new(indices: &'a [u32]) -> Self {
        Self { indices, cursor: 0 }
    }
}

impl Iterator for IndexSpans<'_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.indices.len() {
            return None;
        }

        let first = self.indices[self.cursor];
        let mut last = first;

        let (left, right) = loop {
            self.cursor += 1;
            match self.indices.get(self.cursor) {
                Some(next) if *next == last + 1 => {
                    last += 1;
                }
                _ => break (first, last),
            }
        };
        Some((left as _, right as _))
    }
}

#[cfg(test)]
mod tests;
