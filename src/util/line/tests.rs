use std::iter::once;

use crossterm::style::{ContentStyle, Stylize};

use super::*;
use crate::{
    rect::{Area, ClearState, CrosstermRect},
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

    for text in ["a".repeat(1_000_000), "界".repeat(1_000_000)] {
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
    let mut output = Vec::new();
    let mut rect = CrosstermRect::new(
        &mut output,
        Area {
            column: 5,
            width: 3,
            height: 1,
            ..Area::default()
        },
        ClearState::Precleared,
    )
    .unwrap();
    let spans = once(ContentStyle::new().apply("abc"))
        .chain(std::iter::from_fn(|| panic!("visited a hidden style span")));
    let remaining =
        print_line::<UnicodeProcessor, _>(&mut rect, "abcdef", spans, 0, 3, '~').unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(String::from_utf8(output).unwrap(), "ab~");
}

#[test]
fn scrolling_and_styles_can_cross_different_grapheme_boundaries() {
    let mut output = Vec::new();
    let mut rect = CrosstermRect::new(
        &mut output,
        Area {
            width: 5,
            height: 1,
            ..Area::default()
        },
        ClearState::Precleared,
    )
    .unwrap();
    let spans = [
        "a👩".red(),
        "🏽‍".blue(),
        "💻b".green(),
        ContentStyle::new().apply("c"),
    ];
    let remaining =
        print_line::<UnicodeProcessor, _>(&mut rect, "a👩🏽‍💻bc", spans.into_iter(), 1, 5, '…')
            .unwrap();
    assert_eq!(remaining, 1);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("……{}c", "b".green())
    );
}

#[test]
fn zero_capacity_does_not_visit_styles() {
    let mut output = Vec::new();
    let mut rect = CrosstermRect::new(
        &mut output,
        Area {
            width: 1,
            height: 1,
            ..Area::default()
        },
        ClearState::Precleared,
    )
    .unwrap();
    let spans = std::iter::from_fn(|| panic!("visited a style without space to print"));
    assert_eq!(
        print_line::<UnicodeProcessor, _>(&mut rect, "a", spans, 0, 0, '…').unwrap(),
        0
    );
    assert!(output.is_empty());
}
