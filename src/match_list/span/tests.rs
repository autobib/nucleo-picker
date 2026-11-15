use super::*;
use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};

struct All;

impl KeepLines for All {
    fn subslice<'a>(&self, lines: &'a [Range<usize>]) -> &'a [Range<usize>] {
        lines
    }

    fn from_offset(_: u16) -> Self {
        Self
    }
}

#[test]
fn alignment_stops_measuring_once_the_first_highlight_limits_scrolling() {
    let text = "界".repeat(100_000);
    for (first, expected) in [(0, 0), (2, 3)] {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned =
            Spanned::<CountingProcessor>::new(&[first, 99_999], &text, &mut spans, &mut lines, All);
        for padding in [0, 3, u16::MAX] {
            MEASURED_BYTES.set(0);
            assert_eq!(spanned.required_offset(0, padding), 0);
            assert_eq!(MEASURED_BYTES.get(), 0);
            MEASURED_BYTES.set(0);
            assert_eq!(spanned.required_offset(20, padding), expected);
            assert!(MEASURED_BYTES.get() <= 64, "{}", MEASURED_BYTES.get());
        }
    }
}
