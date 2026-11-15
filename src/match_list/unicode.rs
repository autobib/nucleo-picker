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
mod tests {
    use std::slice::from_ref;

    use super::*;
    use crate::util::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe};

    #[test]
    fn crlf_match_indices_produce_the_correct_spans() {
        use nucleo::{
            Config, Matcher, Utf32Str, Utf32String,
            pattern::{CaseMatching, Normalization, Pattern},
        };

        let pattern = Pattern::parse("ab", CaseMatching::Respect, Normalization::Never);
        let mut matcher = Matcher::new(Config::DEFAULT);
        for (rendered, last_index, line_count) in [
            ("a\r\nb", 2, 2),
            ("a\r\n\r\nb", 3, 3),
            ("a\n\r\nb", 3, 3),
            ("a\r\nb\r\n", 2, 3),
            ("a\r\n界b", 3, 2),
            ("a\r\nu\u{0308}b", 3, 2),
        ] {
            let column = Utf32String::from(rendered);
            assert!(matches!(column.slice(..), Utf32Str::Unicode(_)));
            let mut indices = Vec::new();
            assert!(
                pattern
                    .indices(column.slice(..), &mut matcher, &mut indices)
                    .is_some()
            );
            assert_eq!(indices, [0, last_index]);

            let mut spans = Vec::new();
            let mut lines = Vec::new();
            spans_from_indices::<UnicodeProcessor>(&indices, rendered, &mut spans, &mut lines);
            let matched: Vec<_> = spans
                .iter()
                .filter(|span| span.is_match)
                .map(|span| span.range.clone())
                .collect();
            let last_byte = rendered.find('b').unwrap();
            assert_eq!(matched, [0..1, last_byte..last_byte + 1]);
            assert_eq!(lines.len(), line_count);
            assert!(
                spans
                    .iter()
                    .all(|span| !rendered[span.range.clone()].contains(['\r', '\n']))
            );
        }
    }

    #[test]
    fn ascii_processor_eligibility_matches_the_backend() {
        for rendered in [
            "",
            "ab",
            "a\nb",
            "a\rb",
            "\n\r",
            "a\r\nb",
            "\r\n",
            "界",
            "u\u{0308}",
        ] {
            let column = nucleo::Utf32String::from(rendered);
            assert_eq!(
                is_ascii_safe(rendered),
                column.slice(..).is_ascii(),
                "{rendered:?}"
            );
        }
    }

    #[test]
    fn test_spanned() {
        fn assert_matching(
            indices: &[u32],
            input: &str,
            expected_spans: &[(Range<usize>, bool)],
            expected_lines: &[Range<usize>],
        ) {
            let mut spans = Vec::new();
            let mut lines = Vec::new();
            let check = |spans: &[Span], lines: &[Range<usize>]| {
                assert!(
                    spans
                        .iter()
                        .map(|span| (span.range.clone(), span.is_match))
                        .eq(expected_spans.iter().cloned()),
                    "{input:?}: {spans:?}"
                );
                assert_eq!(lines, expected_lines);
            };

            spans_from_indices::<UnicodeProcessor>(indices, input, &mut spans, &mut lines);
            check(&spans, &lines);

            if is_ascii_safe(input) {
                spans_from_indices::<AsciiProcessor>(indices, input, &mut spans, &mut lines);
                check(&spans, &lines);
            }
        }

        // basic test
        assert_matching(&[], "a", &[(0..1, false)], from_ref(&(0..1)));

        // newline
        assert_matching(&[], "\na", &[(1..2, false)], &[0..0, 0..1]);
        assert_matching(&[], "\r\na", &[(2..3, false)], &[0..0, 0..1]);
        assert_matching(
            &[],
            "a\n\r\nbc",
            &[(0..1, false), (4..6, false)],
            &[0..1, 1..1, 1..2],
        );

        // small edge cases
        assert_matching(&[], "", &[], from_ref(&(0..0)));
        assert_matching(&[], "\n", &[], &[0..0, 0..0]);
        assert_matching(&[], "\r\n", &[], &[0..0, 0..0]);

        // with indices
        assert_matching(
            &[0, 2],
            "a\nb",
            &[(0..1, true), (2..3, true)],
            &[0..1, 1..2],
        );
        assert_matching(
            &[0, 2],
            "abc",
            &[(0..1, true), (1..2, false), (2..3, true)],
            from_ref(&(0..3)),
        );
        assert_matching(
            &[1, 2, 4, 5, 6],
            "abcdefg",
            &[(0..1, false), (1..3, true), (3..4, false), (4..7, true)],
            from_ref(&(0..4)),
        );
        assert_matching(
            &[2],
            "abcd",
            &[(0..2, false), (2..3, true), (3..4, false)],
            from_ref(&(0..3)),
        );
        assert_matching(
            &[10, 11, 12, 13],
            "abcdefghijklmn",
            &[(0..10, false), (10..14, true)],
            from_ref(&(0..2)),
        );

        // with indices split over newlines
        assert_matching(
            &[0, 2],
            "a\r\nＨ",
            &[(0..1, true), (3..6, true)],
            &[0..1, 1..2],
        );
        assert_matching(
            &[0, 2, 3],
            "abcd\nb",
            &[(0..1, true), (1..2, false), (2..4, true), (5..6, false)],
            &[0..3, 3..4],
        );
    }
}
