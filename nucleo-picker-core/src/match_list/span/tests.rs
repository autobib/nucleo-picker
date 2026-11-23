use super::*;
use crate::component::tests::TestSurface;
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
    let mut rect = TestSurface::new(6, 4);

    MEASURED_BYTES.set(0);
    spanned
        .queue_print(
            &mut rect,
            &mut 0,
            false,
            false,
            0,
            false,
            &PickerChars::new(),
        )
        .unwrap();
    assert_eq!(MEASURED_BYTES.get(), 0);
    let output = rect.rows.join("\n");
    assert!(output.contains("界x"));
    assert!(output.contains("e\u{301}"));
}

#[test]
fn unicode_lines_fit_by_display_width() {
    let text = "界x";
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned = Spanned::<CountingProcessor>::new(&[], text, &mut spans, &mut lines, All);

    for (capacity, fits) in [(3, true), (2, false)] {
        MEASURED_BYTES.set(0);
        assert_eq!(spanned.lines_fit(capacity), fits);
        assert!(MEASURED_BYTES.get() <= text.len());
    }
}

#[test]
fn alignment_stops_measuring_once_the_first_highlight_limits_scrolling() {
    let text = "界".repeat(1_024);
    for (first, expected) in [(0, 0), (2, 3)] {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned =
            Spanned::<CountingProcessor>::new(&[first, 1_023], &text, &mut spans, &mut lines, All);
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
