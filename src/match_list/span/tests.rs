use super::*;
use crate::rect::{Area, ClearState, CrosstermTarget};
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
fn short_unicode_lines_draw_without_measuring_widths() {
    let text = "\n界x\ne\u{301}\r\n";
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned = Spanned::<CountingProcessor>::new(&[], text, &mut spans, &mut lines, All);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(
        Area {
            width: 6,
            height: 4,
            ..Area::default()
        },
        ClearState::Precleared,
    );

    MEASURED_BYTES.set(0);
    spanned
        .queue_print(&mut rect, false, false, 0, false, &PickerChars::new())
        .unwrap();
    assert_eq!(MEASURED_BYTES.get(), 0);
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("界x"));
    assert!(output.contains("e\u{301}"));
}

#[test]
fn lines_with_more_bytes_than_capacity_fall_back_to_measurement() {
    let text = "界x";
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned = Spanned::<CountingProcessor>::new(&[], text, &mut spans, &mut lines, All);

    for (capacity, fits) in [(3, true), (2, false)] {
        MEASURED_BYTES.set(0);
        assert_eq!(spanned.lines_fit(capacity), fits);
        assert_eq!(MEASURED_BYTES.get(), text.len());
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
