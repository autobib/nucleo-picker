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
