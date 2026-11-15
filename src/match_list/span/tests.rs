use super::*;
use crate::util::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe};
use crate::{
    PickerChars,
    rect::{Area, ClearState, CrosstermRect},
};

#[test]
fn required_offset() {
    fn assert_correct_offset(
        indices: &[u32],
        rendered: &str,
        max_width: u16,
        expected_offset: usize,
    ) {
        let mut spans = Vec::new();
        let mut lines = Vec::new();

        let spanned: Spanned<'_, UnicodeProcessor> =
            Spanned::new(indices, rendered, &mut spans, &mut lines, All);
        assert_eq!(spanned.required_offset(max_width, 0), expected_offset);

        if is_ascii_safe(rendered) {
            let spanned: Spanned<'_, AsciiProcessor> =
                Spanned::new(indices, rendered, &mut spans, &mut lines, All);
            assert_eq!(spanned.required_offset(max_width, 0), expected_offset);
        }
    }

    assert_correct_offset(&[5], "abcdef", 4, 2);
    assert_correct_offset(&[5], "abcdef", 5, 1);
    assert_correct_offset(&[], "a", 1, 0);
    assert_correct_offset(&[], "abc", 1, 0);
    assert_correct_offset(&[2], "abc", 1, 1);
    assert_correct_offset(&[2], "abc", 2, 1);
    assert_correct_offset(&[2], "abc", 3, 0);
    assert_correct_offset(&[2], "abc\nab", 2, 1);
    assert_correct_offset(&[7], "abc\nabcd", 2, 2);

    assert_correct_offset(&[0, 7], "abc\nabcd", 2, 0);
    assert_correct_offset(&[1, 7], "abc\nabcd", 2, 0);
    assert_correct_offset(&[2, 7], "abc\nabcd", 2, 1);

    assert_correct_offset(&[0, 6], "abc\naＨd", 2, 0);
    assert_correct_offset(&[1, 6], "abc\naＨd", 2, 0);
    assert_correct_offset(&[2, 6], "abc\naＨd", 2, 1);
    assert_correct_offset(&[2, 6], "abc\naＨd", 3, 1);

    assert_correct_offset(&[2, 4, 8], "abc\na\r\naＨd", 1, 0);
    assert_correct_offset(&[2, 4, 8], "abc\na\r\naＨd", 2, 0);
    assert_correct_offset(&[2, 8], "abc\na\r\naＨd", 2, 1);
    assert_correct_offset(&[2, 4, 8], "abc\na\r\naＨd", 3, 0);
    assert_correct_offset(&[2, 8], "abc\na\r\naＨd", 3, 1);
    assert_correct_offset(&[2, 8], "abc\na\r\naＨd", 4, 0);
}

#[test]
fn alignment_stops_measuring_once_the_first_highlight_limits_scrolling() {
    use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};

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

fn render_lines<P: Processor>(
    rendered: &str,
    area: Area,
    hovered: bool,
    highlight_line: bool,
    clear_mode: ClearState,
) -> String {
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned = Spanned::<P>::new(&[], rendered, &mut spans, &mut lines, All);
    let mut output = Vec::new();

    spanned
        .queue_print(
            &mut CrosstermRect::new(&mut output, area, clear_mode).unwrap(),
            hovered,
            false,
            0,
            highlight_line,
            &PickerChars::new(),
        )
        .unwrap();

    String::from_utf8(output).unwrap()
}

#[test]
fn prefix_and_trailing_columns_respect_width_highlight_and_clear_modes() {
    let area = Area {
        width: 8,
        height: 1,
        ..Area::default()
    };
    assert_eq!(
        render_lines::<AsciiProcessor>(
            "item",
            Area { width: 1, ..area },
            false,
            false,
            ClearState::Precleared,
        ),
        "\x1b[1G "
    );
    for clear_mode in [ClearState::Precleared, ClearState::WithinRect] {
        for highlight in [false, true] {
            let output = render_lines::<AsciiProcessor>("abc", area, true, highlight, clear_mode);
            assert!(output.ends_with(if highlight {
                "abc   \x1b[0m"
            } else {
                "abc\x1b[0m"
            }));
            if !highlight && clear_mode == ClearState::WithinRect {
                assert!(output.starts_with("\x1b[1G        \x1b[1G"));
            }
        }
    }
    let output = render_lines::<UnicodeProcessor>("界", area, true, true, ClearState::Precleared);
    assert!(output.ends_with("界    \x1b[0m"));
}

#[test]
fn multiline_truncation_and_padding_use_the_rectangle_origin() {
    let output = render_lines::<UnicodeProcessor>(
        "abcdefghi\n界a",
        Area {
            column: 5,
            row: 3,
            width: 7,
            height: 2,
        },
        false,
        false,
        ClearState::WithinRect,
    );
    assert_eq!(
        output,
        "\x1b[6G       \x1b[6G  abcd…\x1b[5;6H\x1b[6G       \x1b[6G  界a",
    );
}

#[test]
fn line_output_preserves_the_content_origin_and_reports_remaining_columns() {
    fn assert_line<P: Processor>(
        rendered: &str,
        indices: &[u32],
        offset: usize,
        capacity: u16,
        expected: &str,
        expected_remaining: u16,
    ) {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned = Spanned::<P>::new(indices, rendered, &mut spans, &mut lines, All);
        let mut output = Vec::new();
        let mut rect = CrosstermRect::new(
            &mut output,
            Area {
                column: 5,
                width: capacity + 2,
                height: 1,
                ..Area::default()
            },
            ClearState::Precleared,
        )
        .unwrap();
        rect.move_to_column(2).unwrap();

        let remaining = spanned
            .queue_print_line(
                &mut rect,
                spanned.lines().next().unwrap(),
                offset,
                capacity,
                &PickerChars::new(),
            )
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            (output.as_str(), remaining),
            (expected, expected_remaining),
            "{rendered:?}, {indices:?}, {offset}, {capacity}: {output:?}"
        );
    }
    for (rendered, indices, offset, capacity, expected, remaining) in [
        ("界a", &[1][..], 1, 1, "\x1b[8G…", 0),
        ("a界b", &[2][..], 1, 2, "\x1b[8G……", 0),
        ("界ab", &[1][..], 1, 2, "\x1b[8G……", 0),
        ("界a", &[][..], 0, 2, "\x1b[8G……", 0),
        ("界a", &[0][..], 0, 2, "\x1b[8G……", 0),
        ("界a", &[1][..], 0, 2, "\x1b[8G……", 0),
        ("abc", &[1][..], 0, 2, "\x1b[8Ga…", 0),
        ("abc", &[][..], 0, 6, "\x1b[8Gabc", 3),
        ("abcdef", &[][..], 0, 3, "\x1b[8Gab…", 0),
        ("界a", &[][..], 0, 6, "\x1b[8G界a", 3),
    ] {
        assert_line::<UnicodeProcessor>(rendered, indices, offset, capacity, expected, remaining);
        if is_ascii_safe(rendered) {
            assert_line::<AsciiProcessor>(rendered, indices, offset, capacity, expected, remaining);
        }
    }
}
