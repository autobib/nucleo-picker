use std::iter::once;

use crate::draw::{Color, Style};

use super::*;
use crate::{
    component::tests::TestSurface,
    util::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe},
};

#[test]
fn clipping_preserves_whole_graphemes_and_fits_the_available_columns() {
    fn assert_clip<P: Processor>(
        text: &str,
        offset: usize,
        capacity: u16,
        expected: &str,
        remaining: u16,
    ) {
        let clip = clip_line::<P>(text, offset, capacity);
        let rendered = format!(
            "{}{}{}",
            "…".repeat(usize::from(clip.leading)),
            &text[clip.visible],
            "…".repeat(usize::from(clip.trailing)),
        );
        assert_eq!(rendered, expected, "{text:?}, {offset}, {capacity}");
        assert_eq!(clip.remaining, remaining);
        assert_eq!(
            UnicodeProcessor::width(&rendered) + usize::from(remaining),
            usize::from(capacity),
        );
    }

    for (text, offset, capacity, expected, remaining) in [
        ("", 0, 5, "", 5),
        ("", usize::MAX, 2, "", 2),
        ("abc", 0, 0, "", 0),
        ("abc", 0, 1, "…", 0),
        ("abc", 0, 2, "a…", 0),
        ("abc", 0, 3, "abc", 0),
        ("abc", 0, 5, "abc", 2),
        ("abc", 1, 2, "…c", 0),
        ("abc", 1, 3, "…c", 1),
        ("abc", 2, 1, "…", 0),
        ("abc", 2, 2, "…", 1),
        ("abc", 3, 3, "…", 2),
        ("abc", usize::MAX, 3, "…", 2),
        ("abcdef", 0, 4, "abc…", 0),
        ("abcdef", 1, 4, "…cd…", 0),
        ("abcdef", 2, 4, "…def", 0),
        ("abcdef", 3, 4, "…ef", 1),
        ("界a", 0, 1, "…", 0),
        ("界a", 0, 2, "……", 0),
        ("界a", 0, 3, "界a", 0),
        ("界界", 0, 3, "界…", 0),
        ("a界b", 1, 2, "……", 0),
        ("a界b", 1, 3, "……b", 0),
        ("a界b", 2, 2, "…b", 0),
        ("ab界d", 1, 4, "…界d", 0),
        ("ab界d", 2, 4, "……d", 1),
        ("界", 1, 5, "…", 4),
        ("e\u{301}👩🏽‍💻", 0, 3, "e\u{301}👩🏽‍💻", 0),
        ("e\u{301}👩🏽‍💻x", 0, 3, "e\u{301}……", 0),
        ("\u{17d8}", 0, 3, "\u{17d8}", 0),
        ("\u{17d8}a", 0, 3, "………", 0),
        ("\u{200b}", 0, 0, "", 0),
        ("\u{200b}", 0, 1, "\u{200b}", 1),
        ("a\u{200b}", 0, 1, "a\u{200b}", 0),
        ("a\u{200b}b", 0, 1, "…", 0),
        ("界\u{200b}a", 0, 2, "……", 0),
        ("لا", 0, 1, "…", 0),
        ("لا", 0, 2, "لا", 0),
        ("لا", 1, 2, "…", 1),
    ] {
        assert_clip::<UnicodeProcessor>(text, offset, capacity, expected, remaining);
        if is_ascii_safe(text) {
            assert_clip::<AsciiProcessor>(text, offset, capacity, expected, remaining);
        }
    }
}

#[test]
fn clipping_does_not_measure_the_hidden_suffix() {
    use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};

    for text in ["a".repeat(65_536), "界".repeat(65_536)] {
        for (offset, capacity, budget) in
            [(0, 20, 64), (100, 20, 400), (usize::MAX, 1, 0), (0, 0, 0)]
        {
            MEASURED_BYTES.set(0);
            let clip = clip_line::<CountingProcessor>(&text, offset, capacity);
            assert_eq!(clip.remaining, 0);
            assert!(clip.leading + clip.trailing > 0 || capacity == 0);
            assert!(MEASURED_BYTES.get() <= budget, "{}", MEASURED_BYTES.get());
        }
    }
}

#[test]
fn styled_output_only_visits_visible_spans_and_never_moves_the_cursor() {
    for (capacity, expected) in [(0, ""), (3, "ab~")] {
        let mut rect = TestSurface::new(3, 1);
        rect.begin_row(0, true).unwrap();
        let spans = once(Style::new().apply("abc"))
            .take(usize::from(capacity != 0))
            .chain(std::iter::from_fn(|| panic!("visited a hidden style span")));
        let remaining = print_line::<UnicodeProcessor, _>(
            &mut rect,
            "abcdef",
            spans,
            0,
            capacity,
            '~',
            Style::new(),
        )
        .unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(rect.rows[0], expected);
        assert_eq!(rect.moves, [(0, 0)]);
    }
}

#[test]
fn scrolling_and_styles_can_cross_different_grapheme_boundaries() {
    let mut rect = TestSurface::new(5, 1);
    rect.begin_row(0, true).unwrap();
    let spans = [
        Style::new().fg(Color::Red).apply("a👩"),
        Style::new().fg(Color::Blue).apply("🏽‍"),
        Style::new().fg(Color::Green).apply("💻b"),
        Style::new().apply("c"),
    ];
    let remaining = print_line::<UnicodeProcessor, _>(
        &mut rect,
        "a👩🏽‍💻bc",
        spans.into_iter(),
        1,
        5,
        '…',
        Style::new(),
    )
    .unwrap();
    assert_eq!(remaining, 1);
    assert_eq!(rect.rows[0], "……bc");
    let styled: Vec<_> = rect
        .spans
        .iter()
        .flat_map(|(text, style)| text.chars().map(move |ch| (ch, *style)))
        .collect();
    assert_eq!(
        styled,
        [
            ('…', Style::new()),
            ('…', Style::new()),
            ('b', Style::new().fg(Color::Green)),
            ('c', Style::new()),
        ],
    );
}
