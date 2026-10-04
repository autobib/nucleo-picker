//! Utilities for handling unicode display in the terminal.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::module_name_repetitions)]

/// A [`Processor`] is an abstraction over the various Unicode operations supported by
/// the [`UnicodeSegmentation`](`unicode_segmentation::UnicodeSegmentation`) and
/// [`UnicodeWidthStr`](unicode_width::UnicodeWidthStr) traits.
///
/// This abstraction is sealed and only has two implementations [`UnicodeProcessor`] and
/// [`AsciiProcessor`].
///
/// A [`UnicodeProcessor`] is a generic processor which accepts any valid UTF-8. In contrast, the
/// [`AsciiProcessor`] only requires ASCII characters with no Windows-style newline `\r\n`. The
/// [`AsciiProcessor`] contains a number of optimizations which handle the ASCII-only case more
/// performatively. This aligns with the upstream handling in Nucleo.
pub trait Processor: private::Sealed {
    /// Compute the width (in terms of visible columns) of the input string.
    ///
    /// This method assumes that `input` is non-empty and does not contain newlines or carriage
    /// returns. If this is not the case, the returned value is undefined.
    fn width(input: &str) -> usize;

    /// Return an iterator over pairs `(offset, grapheme_width)` for the graphemes in `input`.
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)>;

    /// Compute the width (in terms of visible columns) of the last grapheme.
    ///
    /// This method assumes that `input` is non-empty and does not contain a trailing newline. If
    /// this is not the case, the returned value is undefined.
    fn last_grapheme_width(input: &str) -> usize;
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::UnicodeProcessor {}
    impl Sealed for super::AsciiProcessor {}
}

/// Whether or not a given string slice is safe to use with an [`AsciiProcessor`].
#[inline]
pub(crate) fn is_ascii_safe(input: &str) -> bool {
    input.is_ascii() && !input.contains("\r\n")
}

/// A [`Processor`] which is safe for use on all valud UTF-8.
pub struct UnicodeProcessor;

impl Processor for UnicodeProcessor {
    /// Do things properly and use [`UnicodeWidthStr`](unicode_width::UnicodeWidthStr).
    #[inline]
    fn width(input: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(input)
    }

    /// Do things properly and use
    /// [`UnicodeSegmentation`](unicode_segmentation::UnicodeSegmentation).
    #[inline]
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(input, true)
            .map(|(offset, grapheme)| (offset, unicode_width::UnicodeWidthStr::width(grapheme)))
    }

    /// Do things properly and use
    /// [`UnicodeSegmentation`](unicode_segmentation::UnicodeSegmentation) as well as
    /// [`UnicodeWidthStr`](unicode_width::UnicodeWidthStr).
    #[inline]
    fn last_grapheme_width(input: &str) -> usize {
        unicode_segmentation::UnicodeSegmentation::graphemes(input, true)
            .next_back()
            .map_or(0, unicode_width::UnicodeWidthStr::width)
    }
}

pub struct AsciiProcessor;

impl Processor for AsciiProcessor {
    /// Since we assume there are no carriage returns and no newlines, the width of a string is
    /// just the number of bytes.
    #[inline]
    fn width(input: &str) -> usize {
        debug_assert!(is_ascii_safe(input));
        input.len()
    }

    #[inline]
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
        debug_assert!(is_ascii_safe(input));
        std::iter::repeat_n(1, input.len()).enumerate()
    }

    #[inline]
    fn last_grapheme_width(input: &str) -> usize {
        debug_assert!(is_ascii_safe(input));
        1
    }
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
pub fn truncate<P: Processor>(input: &str, capacity: u16) -> Result<u16, (&str, usize)> {
    if let Some(remaining) = (capacity as usize).checked_sub(P::width(input)) {
        Ok(remaining as u16)
    } else {
        let mut current_length = 0;
        for (offset, grapheme_width) in P::grapheme_index_widths(input) {
            let next_length = current_length + grapheme_width;
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
pub fn consume<P: Processor>(input: &str, offset: usize) -> (usize, usize) {
    let mut initial_width: usize = 0;
    for (idx, grapheme_width) in P::grapheme_index_widths(input) {
        match initial_width.checked_sub(offset) {
            Some(diff) => return (idx, diff),
            None => initial_width += grapheme_width,
        }
    }
    (input.len(), initial_width.saturating_sub(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consume_offset() {
        fn assert_consume(input: &str, w: usize, expected: (usize, usize)) {
            assert_eq!(consume::<UnicodeProcessor>(input, w), expected);

            if is_ascii_safe(input) {
                assert_eq!(consume::<AsciiProcessor>(input, w), expected);
            }
        }
        assert_consume("ab", 3, (2, 0));
        assert_consume("ab", 2, (2, 0));
        assert_consume("ab", 1, (1, 0));
        assert_consume("ab", 0, (0, 0));
        assert_consume("", 0, (0, 0));
        assert_consume("", 1, (0, 0));

        assert_consume("Ｈ", 0, (0, 0));
        assert_consume("Ｈ", 1, (3, 1));
        assert_consume("Ｈ", 2, (3, 0));

        assert_consume("aＨ", 0, (0, 0));
        assert_consume("aＨ", 1, (1, 0));
        assert_consume("aＨ", 2, (4, 1));
        assert_consume("aＨ", 3, (4, 0));
    }

    #[test]
    fn test_truncate_width() {
        fn assert_truncate(input: &str, w: u16, expected: Result<u16, (&str, usize)>) {
            assert_eq!(truncate::<UnicodeProcessor>(input, w), expected);
            if is_ascii_safe(input) {
                assert_eq!(truncate::<AsciiProcessor>(input, w), expected);
            }
        }

        assert_truncate("", 0, Ok(0));

        assert_truncate("ab", 0, Err(("", 0)));
        assert_truncate("ab", 1, Err(("a", 0)));
        assert_truncate("ab", 2, Ok(0));

        assert_truncate("Ｈｅ", 0, Err(("", 0)));
        assert_truncate("Ｈｅ", 1, Err(("", 1)));
        assert_truncate("Ｈｅ", 2, Err(("Ｈ", 0)));
        assert_truncate("Ｈｅ", 3, Err(("Ｈ", 1)));
        assert_truncate("Ｈｅ", 4, Ok(0));
        assert_truncate("Ｈｅ", 5, Ok(1));

        assert_truncate("aＨ", 1, Err(("a", 0)));
        assert_truncate("aＨ", 2, Err(("a", 1)));
        assert_truncate("aＨ", 3, Ok(0));
        assert_truncate("aＨ", 4, Ok(1));
    }
}
