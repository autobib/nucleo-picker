//! Utilities for handling unicode display in the terminal.

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
    use std::iter::once;

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
            indices: Vec<u32>,
            input: &'static str,
            expected_spans: Vec<Span>,
            expected_lines: Vec<Range<usize>>,
        ) {
            let mut spans = Vec::new();
            let mut lines = Vec::new();

            spans_from_indices::<UnicodeProcessor>(&indices, input, &mut spans, &mut lines);
            assert_eq!(spans, expected_spans);
            assert_eq!(lines, expected_lines);

            if is_ascii_safe(input) {
                spans_from_indices::<AsciiProcessor>(&indices, input, &mut spans, &mut lines);
                assert_eq!(spans, expected_spans);
                assert_eq!(lines, expected_lines);
            }
        }

        // basic test
        assert_matching(
            Vec::new(),
            "a",
            vec![Span {
                range: 0..1,
                is_match: false,
            }],
            once(0..1).collect(),
        );

        // newline
        assert_matching(
            Vec::new(),
            "\na",
            vec![Span {
                range: 1..2,
                is_match: false,
            }],
            vec![0..0, 0..1],
        );
        assert_matching(
            Vec::new(),
            "\r\na",
            vec![Span {
                range: 2..3,
                is_match: false,
            }],
            vec![0..0, 0..1],
        );
        assert_matching(
            Vec::new(),
            "a\n\r\nbc",
            vec![
                Span {
                    range: 0..1,
                    is_match: false,
                },
                Span {
                    range: 4..6,
                    is_match: false,
                },
            ],
            vec![0..1, 1..1, 1..2],
        );

        // small edge cases
        assert_matching(Vec::new(), "", vec![], once(0..0).collect());
        assert_matching(Vec::new(), "\n", vec![], vec![0..0, 0..0]);
        assert_matching(Vec::new(), "\r\n", vec![], vec![0..0, 0..0]);

        // with indices
        assert_matching(
            vec![0, 2],
            "a\nb",
            vec![
                Span {
                    range: 0..1,
                    is_match: true,
                },
                Span {
                    range: 2..3,
                    is_match: true,
                },
            ],
            vec![0..1, 1..2],
        );
        assert_matching(
            vec![0, 2],
            "abc",
            vec![
                Span {
                    range: 0..1,
                    is_match: true,
                },
                Span {
                    range: 1..2,
                    is_match: false,
                },
                Span {
                    range: 2..3,
                    is_match: true,
                },
            ],
            once(0..3).collect(),
        );

        // with indices split over newlines
        assert_matching(
            vec![0, 2],
            "a\r\nＨ",
            vec![
                Span {
                    range: 0..1,
                    is_match: true,
                },
                Span {
                    range: 3..6,
                    is_match: true,
                },
            ],
            vec![0..1, 1..2],
        );
        assert_matching(
            vec![0, 2, 3],
            "abcd\nb",
            vec![
                Span {
                    range: 0..1,
                    is_match: true,
                },
                Span {
                    range: 1..2,
                    is_match: false,
                },
                Span {
                    range: 2..4,
                    is_match: true,
                },
                Span {
                    range: 5..6,
                    is_match: false,
                },
            ],
            vec![0..3, 3..4],
        );
    }

    #[test]
    fn test_next_span() {
        let indices: Vec<u32> = vec![1, 2, 4, 5, 6];
        let mut is = IndexSpans::new(&indices);
        assert_eq!(is.next(), Some((1, 2)));
        assert_eq!(is.cursor, 2);
        assert_eq!(is.next(), Some((4, 6)));
        assert_eq!(is.cursor, 5);
        assert_eq!(is.next(), None);
        assert_eq!(is.cursor, 5);

        let indices: Vec<u32> = vec![];
        let mut is = IndexSpans::new(&indices);
        assert_eq!(is.next(), None);
        assert_eq!(is.cursor, 0);

        let indices: Vec<u32> = vec![2];
        let mut is = IndexSpans::new(&indices);
        assert_eq!(is.next(), Some((2, 2)));
        assert_eq!(is.cursor, 1);
        assert_eq!(is.next(), None);
        assert_eq!(is.cursor, 1);

        let indices: Vec<u32> = vec![10, 11, 12, 13];
        let mut is = IndexSpans::new(&indices);
        assert_eq!(is.next(), Some((10, 13)));
        assert_eq!(is.cursor, 4);
        assert_eq!(is.next(), None);
        assert_eq!(is.cursor, 4);
    }
}
