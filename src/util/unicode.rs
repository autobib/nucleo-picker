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

    /// Returns min(width, limit), stopping once the limit is reached.
    fn width_up_to(input: &str, limit: usize) -> usize {
        if limit == 0 {
            return 0;
        }
        let mut total = 0;
        for (_, width) in Self::grapheme_index_widths(input) {
            if width >= limit - total {
                return limit;
            }
            total += width;
        }
        total
    }

    /// Return an iterator over pairs `(offset, grapheme_width)` for the graphemes in `input`.
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)>;
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
    #[inline]
    fn width(input: &str) -> usize {
        // Sum individual graphemes for consistency with clipping and horizontal alignment.
        Self::grapheme_index_widths(input)
            .map(|(_, width)| width)
            .sum()
    }

    /// Do things properly and use
    /// [`UnicodeSegmentation`](unicode_segmentation::UnicodeSegmentation).
    #[inline]
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(input, true)
            .map(|(offset, grapheme)| (offset, unicode_width::UnicodeWidthStr::width(grapheme)))
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
    fn width_up_to(input: &str, limit: usize) -> usize {
        input.len().min(limit)
    }

    #[inline]
    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
        debug_assert!(is_ascii_safe(input));
        std::iter::repeat_n(1, input.len()).enumerate()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        pub(crate) static MEASURED_BYTES: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) struct CountingProcessor;

    impl private::Sealed for CountingProcessor {}

    impl Processor for CountingProcessor {
        fn width(input: &str) -> usize {
            MEASURED_BYTES.update(|count| count + input.len());
            UnicodeProcessor::width(input)
        }

        fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
            unicode_segmentation::UnicodeSegmentation::grapheme_indices(input, true).map(
                |(offset, grapheme)| {
                    MEASURED_BYTES.update(|count| count + grapheme.len());
                    (offset, UnicodeProcessor::width(grapheme))
                },
            )
        }
    }

    #[test]
    fn bounded_width_stops_at_the_limit() {
        let text = "界".repeat(1_000_000);
        MEASURED_BYTES.set(0);
        assert_eq!(CountingProcessor::width_up_to(&text, 21), 21);
        assert!(MEASURED_BYTES.get() <= 33);
        MEASURED_BYTES.set(0);
        assert_eq!(CountingProcessor::width_up_to(&text, 0), 0);
        assert_eq!(MEASURED_BYTES.get(), 0);
        assert_eq!(UnicodeProcessor::width_up_to("界a", 10), 3);
        assert_eq!(UnicodeProcessor::width_up_to("", 10), 0);
        assert_eq!(AsciiProcessor::width_up_to("abc", 2), 2);
    }

    #[test]
    fn width_uses_grapheme_advances() {
        assert_eq!(UnicodeProcessor::width("لا"), 2);
        assert_eq!(UnicodeProcessor::width("e\u{301}👩🏽‍💻"), 3);
    }
}
