//! Utilities for handling Unicode display in the terminal.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::module_name_repetitions)]

/// Grapheme segmentation.
///
/// Width calculations are provided directly by the drawing target independently of grapheme
/// segmentation.
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
    /// The input must not contain control characters or newlines. The width is computed by summing
    /// over the widths of extended graphemes.
    fn width(input: &str, width: impl Fn(&str) -> usize) -> usize {
        Self::grapheme_index_widths(input, width)
            .map(|(_, width)| width)
            .sum()
    }

    /// Returns min(width, limit), stopping once the limit is reached.
    fn width_up_to(input: &str, limit: usize, width: impl Fn(&str) -> usize) -> usize {
        if limit == 0 {
            return 0;
        }
        let mut total = 0;
        for (_, width) in Self::grapheme_index_widths(input, width) {
            if width >= limit - total {
                return limit;
            }
            total += width;
        }
        total
    }

    /// Return graphemes and byte offsets (without width computations).
    fn grapheme_indices(input: &str) -> impl DoubleEndedIterator<Item = (usize, &str)>;

    /// Return an iterator over pairs `(offset, grapheme_width)` for the graphemes in `input`.
    fn grapheme_index_widths(
        input: &str,
        width: impl Fn(&str) -> usize,
    ) -> impl Iterator<Item = (usize, usize)> {
        Self::grapheme_indices(input).map(move |(offset, grapheme)| (offset, width(grapheme)))
    }
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

/// Default grapheme width.
#[inline]
pub(crate) fn grapheme_width(grapheme: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(grapheme)
}

/// A [`Processor`] which is safe for use on all valid UTF-8.
pub struct UnicodeProcessor;

impl Processor for UnicodeProcessor {
    /// Do things properly and use
    /// [`UnicodeSegmentation`](unicode_segmentation::UnicodeSegmentation).
    #[inline]
    fn grapheme_indices(input: &str) -> impl DoubleEndedIterator<Item = (usize, &str)> {
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(input, true)
    }
}

pub struct AsciiProcessor;

impl Processor for AsciiProcessor {
    /// Since we assume there are no carriage returns and no newlines, the width of a string is
    /// just the number of bytes.
    #[inline]
    fn width(input: &str, _width: impl Fn(&str) -> usize) -> usize {
        debug_assert!(is_ascii_safe(input));
        input.len()
    }

    #[inline]
    fn width_up_to(input: &str, limit: usize, _width: impl Fn(&str) -> usize) -> usize {
        input.len().min(limit)
    }

    #[inline]
    fn grapheme_indices(input: &str) -> impl DoubleEndedIterator<Item = (usize, &str)> {
        debug_assert!(is_ascii_safe(input));
        (0..input.len()).map(|offset| (offset, &input[offset..offset + 1]))
    }

    #[inline]
    fn grapheme_index_widths(
        input: &str,
        _width: impl Fn(&str) -> usize,
    ) -> impl Iterator<Item = (usize, usize)> {
        debug_assert!(is_ascii_safe(input));
        std::iter::repeat_n(1, input.len()).enumerate()
    }
}

#[cfg(test)]
pub(crate) mod tests;
