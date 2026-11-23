use super::*;
use crate::component::tests::TestSurface;
use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};
use crate::util::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe};

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

#[test]
fn highlight_visibility_accounts_for_padding_and_trailing_content() {
    fn assert_rendered<P: Processor>(
        text: &str,
        indices: &[u32],
        width: u16,
        padding: u16,
        expected: &[&str],
    ) {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned = Spanned::<P>::new(indices, text, &mut spans, &mut lines, All);
        let mut surface = TestSurface::new(width, expected.len() as u16);
        spanned
            .queue_print(
                &mut surface,
                &mut 0,
                true,
                false,
                padding,
                false,
                &PickerChars::new(),
            )
            .unwrap();
        assert_eq!(surface.rows, expected, "{text:?}, {width}, {padding}");
    }

    for (text, indices, width, padding, expected) in [
        ("abcdeXyz", &[5][..], 5, 0, &["▌ …X…"][..]),
        ("abcdeX", &[5], 5, 0, &["▌ …eX"]),
        ("abcdeX\u{200b}", &[5], 5, 0, &["▌ …eX\u{200b}"]),
        ("abcde界yz", &[5], 6, 0, &["▌ …界…"]),
        ("界界Xyz", &[2], 5, 0, &["▌ …X…"]),
        ("abcdee\u{301}yz", &[5], 5, 0, &["▌ …e\u{301}…"]),
        ("abcdeXyz", &[5], 7, 2, &["▌ …eXyz"]),
        ("abcdeXyz", &[5], 7, u16::MAX, &["▌ …Xyz"]),
        ("abc", &[], 4, 3, &["▌ a…"]),
        ("abcdXyz\nabcdeYuv", &[4, 13], 6, 0, &["▌ …Xyz", "▌ …eY…"]),
        ("abcdeYuv\nabcdXyz", &[5, 13], 5, 0, &["▌ …e…", "▌ …X…"]),
    ] {
        assert_rendered::<UnicodeProcessor>(text, indices, width, padding, expected);
        if is_ascii_safe(text) {
            assert_rendered::<AsciiProcessor>(text, indices, width, padding, expected);
        }
    }
}
