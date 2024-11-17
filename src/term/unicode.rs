//! Utilities for handling unicode display in the terminal.

#![allow(clippy::cast_possible_truncation)]

use std::ops::Range;

use memchr::memchr_iter;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A span corresponding to an unowned sub-slice of a string.
#[derive(Debug, PartialEq)]
pub struct Span {
    pub range: Range<usize>,
    pub is_match: bool,
}

/// Attempt to fit `input` into `capacity` columns.
///
/// - The `Ok` variant indicates that the input fit into the desired capacity and contains the
///   remaining capicity.
/// - The `Err` variant indicates that there was not enough space, and contais a pair `(prefix,
///   alignment`). Here, `prefix` is the maximal prefix of `input` composed of full graphemes
///   which fits inside the provided capacity, and `alignment` is the remaining capacity which
///   could not be written into because the next grapheme was too long.
///
/// Note that this call is meaningful even when `capacity == 0`, since the width of the input is in
/// terms of unicode width as computed by [`UnicodeWidthStr`], and therefore may be 0 even for
/// non-empty string slices such as `\u{200b}`.
#[inline]
pub fn truncate(input: &str, capacity: u16) -> Result<u16, (&str, usize)> {
    if let Some(remaining) = (capacity as usize).checked_sub(input.width()) {
        Ok(remaining as u16)
    } else {
        let mut current_length = 0;
        for (offset, grapheme) in input.grapheme_indices(true) {
            let next_length = current_length + grapheme.width();
            if next_length > capacity as usize {
                return Err((&input[..offset], capacity as usize - current_length));
            }
            current_length = next_length;
        }

        Ok(capacity - current_length as u16)
    }
}

/// Consume a prefix consisting of entire graphemes from `input` until the total length of the
/// consumed graphemes exceeds `offset`. Returns a pair `(idx, alignment)` where `idx` is the
/// byte index of the first valid grapheme, and `alignment` is the number of extra columns
/// resulting from rounding to the nearest grapheme.
///
/// Usually `alignment == 0`, but in the presence of (for instance) double-width characters such as
/// `Ｈ` it could be larger.
#[inline]
pub fn consume(input: &str, offset: usize) -> (usize, usize) {
    let mut initial_width: usize = 0;
    for (idx, grapheme) in input.grapheme_indices(true) {
        match initial_width.checked_sub(offset) {
            Some(diff) => return (idx, diff),
            None => initial_width += grapheme.width(),
        }
    }
    (input.len(), initial_width.saturating_sub(offset))
}

/// Compute `spans` and `lines` corresponding to the provided indices in the given buffers.
///
/// Note that this will automatically clear the buffers.
///
/// The `spans` are guaranteed to not contain newlines. In order to determine which spans belong to
/// which line, `lines` consists of contiguous sub-slices of `spans`.
#[inline]
pub fn spans_from_indices(
    indices: &[u32],
    rendered: &str,
    spans: &mut Vec<Span>,
    lines: &mut Vec<Range<usize>>,
) {
    spans.clear();
    lines.clear();

    let mut grapheme_index_iter = rendered.grapheme_indices(true);

    let mut iter_step_count = 0; // how many graphemes we have consumed
    let mut start = 0; // the current offset position for the next block
    let mut line_start = 0;
    let mut line_end = 0;

    let mut cursor = 0; // only used for internal state of `next_span`
    while let Some((left, right)) = next_span(&mut cursor, indices) {
        let (middle, _) = grapheme_index_iter
            .nth(left - iter_step_count)
            .expect("Match index does not correspond to grapheme!");
        let end = if let Some((end, _)) = grapheme_index_iter.nth(right - left) {
            // + 2, since `nth` is zero-indexed and we called it twice
            iter_step_count = right + 2;
            end
        } else {
            rendered.len()
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

/// Given a set of indices, return the next span if any. A span is a pair `(usize, usize)`,
/// corresponding to grapheme indices.
#[inline]
fn next_span(cursor: &mut usize, indices: &[u32]) -> Option<(usize, usize)> {
    if *cursor >= indices.len() {
        return None;
    }

    let first = indices[*cursor];
    let mut last = first;

    let (left, right) = loop {
        *cursor += 1;
        match indices.get(*cursor) {
            Some(next) => {
                if *next == last + 1 {
                    last += 1;
                } else {
                    break (first, last);
                }
            }
            None => {
                break (first, last);
            }
        }
    };
    Some((left as _, right as _))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consume_offset() {
        assert_eq!(consume("ab", 3), (2, 0));
        assert_eq!(consume("ab", 2), (2, 0));
        assert_eq!(consume("ab", 1), (1, 0));
        assert_eq!(consume("ab", 0), (0, 0));
        assert_eq!(consume("", 0), (0, 0));
        assert_eq!(consume("", 1), (0, 0));

        assert_eq!(consume("Ｈ", 0), (0, 0));
        assert_eq!(consume("Ｈ", 1), (3, 1));
        assert_eq!(consume("Ｈ", 2), (3, 0));

        assert_eq!(consume("aＨ", 0), (0, 0));
        assert_eq!(consume("aＨ", 1), (1, 0));
        assert_eq!(consume("aＨ", 2), (4, 1));
        assert_eq!(consume("aＨ", 3), (4, 0));
    }

    #[test]
    fn test_spanned() {
        fn assert_matching_vecs<T: std::fmt::Debug + PartialEq>(a: &Vec<T>, b: &Vec<T>) {
            for (u, v) in a.iter().zip(b.iter()) {
                assert_eq!(u, v);
            }
        }

        fn assert_matching(
            indices: Vec<u32>,
            input: &'static str,
            expected_spans: Vec<Span>,
            expected_lines: Vec<Range<usize>>,
        ) {
            let mut spans = Vec::new();
            let mut lines = Vec::new();
            spans_from_indices(&indices, &input, &mut spans, &mut lines);
            assert_matching_vecs(&spans, &expected_spans);
            assert_matching_vecs(&lines, &expected_lines);
        }

        // basic test
        assert_matching(
            Vec::new(),
            "a",
            vec![Span {
                range: 0..1,
                is_match: false,
            }],
            vec![0..1],
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
        assert_matching(Vec::new(), "", vec![], vec![0..0]);
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
            vec![0..3],
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
        let mut cursor: usize = 0;
        assert_eq!(next_span(&mut cursor, &indices), Some((1, 2)));
        assert_eq!(cursor, 2);
        assert_eq!(next_span(&mut cursor, &indices), Some((4, 6)));
        assert_eq!(cursor, 5);
        assert_eq!(next_span(&mut cursor, &indices), None);
        assert_eq!(cursor, 5);

        let indices: Vec<u32> = vec![];
        let mut cursor: usize = 0;
        assert_eq!(next_span(&mut cursor, &indices), None);
        assert_eq!(cursor, 0);

        let indices: Vec<u32> = vec![2];
        let mut cursor: usize = 0;
        assert_eq!(next_span(&mut cursor, &indices), Some((2, 2)));
        assert_eq!(cursor, 1);
        assert_eq!(next_span(&mut cursor, &indices), None);
        assert_eq!(cursor, 1);

        let indices: Vec<u32> = vec![10, 11, 12, 13];
        let mut cursor: usize = 0;
        assert_eq!(next_span(&mut cursor, &indices), Some((10, 13)));
        assert_eq!(cursor, 4);
        assert_eq!(next_span(&mut cursor, &indices), None);
        assert_eq!(cursor, 4);
    }

    #[test]
    fn test_truncate_width() {
        assert_eq!(truncate("", 0), Ok(0));

        assert_eq!(truncate("ab", 0), Err(("", 0)));
        assert_eq!(truncate("ab", 1), Err(("a", 0)));
        assert_eq!(truncate("ab", 2), Ok(0));

        assert_eq!(truncate("Ｈｅ", 0), Err(("", 0)));
        assert_eq!(truncate("Ｈｅ", 1), Err(("", 1)));
        assert_eq!(truncate("Ｈｅ", 2), Err(("Ｈ", 0)));
        assert_eq!(truncate("Ｈｅ", 3), Err(("Ｈ", 1)));
        assert_eq!(truncate("Ｈｅ", 4), Ok(0));
        assert_eq!(truncate("Ｈｅ", 5), Ok(1));

        assert_eq!(truncate("aＨ", 1), Err(("a", 0)));
        assert_eq!(truncate("aＨ", 2), Err(("a", 1)));
        assert_eq!(truncate("aＨ", 3), Ok(0));
        assert_eq!(truncate("aＨ", 4), Ok(1));
    }
}
