use std::cell::Cell;

use super::*;

thread_local! {
    pub(crate) static MEASURED_BYTES: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct CountingProcessor;

impl private::Sealed for CountingProcessor {}

impl Processor for CountingProcessor {
    fn grapheme_indices(input: &str) -> impl DoubleEndedIterator<Item = (usize, &str)> {
        UnicodeProcessor::grapheme_indices(input)
    }

    fn grapheme_index_widths(input: &str) -> impl Iterator<Item = (usize, usize)> {
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(input, true).map(
            |(offset, grapheme)| {
                MEASURED_BYTES.update(|count| count + grapheme.len());
                (offset, grapheme_width(grapheme))
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
fn grapheme_width_is_bounded_by_utf8_length() {
    let check = |text: &str| {
        for (_, grapheme) in UnicodeProcessor::grapheme_indices(text) {
            assert!(
                grapheme_width(grapheme) <= grapheme.len(),
                "width exceeds UTF-8 byte length: {grapheme:?}"
            );
        }
    };

    for ch in ('\0'..=char::MAX).filter(|ch| !ch.is_control()) {
        check(ch.encode_utf8(&mut [0; 4]));
    }
    for text in [
        "e\u{301}",
        "\u{200b}",
        "\u{17d8}",
        "ｶﾞ",
        "ﾊﾟ",
        "aﾞ",
        "界ﾞﾟ",
        "\u{ff9e}\u{ff9f}\u{ff9e}",
        "1\u{fe0f}\u{20e3}",
        "©\u{fe0f}",
        "“\u{fe01}",
        "👩🏽‍💻",
        "🇨🇦",
        "لا",
        "\u{1780}\u{17d2}\u{1780}",
    ] {
        check(text);
    }
}
