use super::{
    super::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe, is_unicode_safe},
    *,
};
use crate::PickerChars;

#[test]
fn required_width() {
    fn assert_correct_width(indices: Vec<u32>, rendered: &str, expected_width: usize) {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned: Spanned<'_, UnicodeProcessor> =
            Spanned::new(&indices, rendered, &mut spans, &mut lines, All);

        if is_unicode_safe(rendered) {
            assert_eq!(spanned.required_width(), expected_width);
        }

        if is_ascii_safe(rendered) {
            let spanned: Spanned<'_, AsciiProcessor> =
                Spanned::new(&indices, rendered, &mut spans, &mut lines, All);
            assert_eq!(spanned.required_width(), expected_width);
        }
    }

    assert_correct_width(vec![], "a", 0);
    assert_correct_width(vec![0], "a", 1);
    assert_correct_width(vec![1], "ab", 2);
    assert_correct_width(vec![0], "Ｈb", 2);
    assert_correct_width(vec![1], "Ｈb", 3);

    assert_correct_width(vec![0, 4], "ab\ncd", 2);
    assert_correct_width(vec![0, 4], "ab\nＨd", 3);
    assert_correct_width(vec![0, 5], "ab\n\nＨＨ", 4);
    assert_correct_width(vec![1, 5], "ＨＨb\n\nab", 4);
}

#[test]
fn required_offset() {
    fn assert_correct_offset(
        indices: Vec<u32>,
        rendered: &str,
        max_width: u16,
        expected_offset: usize,
    ) {
        let mut spans = Vec::new();
        let mut lines = Vec::new();

        if is_unicode_safe(rendered) {
            let spanned: Spanned<'_, UnicodeProcessor> =
                Spanned::new(&indices, rendered, &mut spans, &mut lines, All);
            assert_eq!(spanned.required_offset(max_width, 0), expected_offset);
        }

        if is_ascii_safe(rendered) {
            let spanned: Spanned<'_, AsciiProcessor> =
                Spanned::new(&indices, rendered, &mut spans, &mut lines, All);
            assert_eq!(spanned.required_offset(max_width, 0), expected_offset);
        }
    }

    assert_correct_offset(vec![], "a", 1, 0);
    assert_correct_offset(vec![], "abc", 1, 0);
    assert_correct_offset(vec![2], "abc", 1, 2);
    assert_correct_offset(vec![2], "abc", 2, 2);
    assert_correct_offset(vec![2], "abc", 3, 0);
    assert_correct_offset(vec![2], "abc\nab", 2, 2);
    assert_correct_offset(vec![7], "abc\nabcd", 2, 3);

    assert_correct_offset(vec![7], "abc\nabcd", 2, 3);

    assert_correct_offset(vec![0, 7], "abc\nabcd", 2, 0);
    assert_correct_offset(vec![1, 7], "abc\nabcd", 2, 0);
    assert_correct_offset(vec![2, 7], "abc\nabcd", 2, 2);

    assert_correct_offset(vec![0, 6], "abc\naＨd", 2, 0);
    assert_correct_offset(vec![1, 6], "abc\naＨd", 2, 0);
    assert_correct_offset(vec![2, 6], "abc\naＨd", 2, 2);
    assert_correct_offset(vec![2, 6], "abc\naＨd", 3, 2);

    assert_correct_offset(vec![2, 4, 8], "abc\na\r\naＨd", 1, 0);
    assert_correct_offset(vec![2, 4, 8], "abc\na\r\naＨd", 2, 0);
    assert_correct_offset(vec![2, 8], "abc\na\r\naＨd", 2, 2);
    assert_correct_offset(vec![2, 4, 8], "abc\na\r\naＨd", 3, 0);
    assert_correct_offset(vec![2, 8], "abc\na\r\naＨd", 3, 2);
    assert_correct_offset(vec![2, 8], "abc\na\r\naＨd", 4, 0);
}

#[test]
fn line_prefix_does_not_exceed_the_available_width() {
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned: Spanned<'_, AsciiProcessor> =
        Spanned::new(&[], "item", &mut spans, &mut lines, All);
    let mut output = Vec::new();

    spanned
        .queue_print(
            &mut output,
            false,
            false,
            1,
            0,
            false,
            ClearMode::All,
            &PickerChars::new(),
        )
        .unwrap();

    assert!(output.starts_with(b" "));
    assert!(!output.starts_with(b"  "));
    assert!(!String::from_utf8(output).unwrap().contains("\x1b[K"));
}

fn render_ascii_line(
    rendered: &str,
    width: u16,
    selected: bool,
    highlight_line: bool,
    clear_mode: ClearMode,
) -> String {
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned: Spanned<'_, AsciiProcessor> =
        Spanned::new(&[], rendered, &mut spans, &mut lines, All);
    let mut output = Vec::new();

    spanned
        .queue_print(
            &mut output,
            selected,
            false,
            width,
            0,
            highlight_line,
            clear_mode,
            &PickerChars::new(),
        )
        .unwrap();

    String::from_utf8(output).unwrap()
}

#[test]
fn trailing_columns_follow_the_highlight_and_clear_modes() {
    let default = render_ascii_line("abc", 8, true, false, ClearMode::All);
    assert!(default.contains("abc\x1b[0m"));
    assert!(!default.contains("abc \x1b[0m"));

    let highlighted = render_ascii_line("abc", 8, true, true, ClearMode::All);
    assert!(highlighted.contains("abc   \x1b[0m"));

    let exact = render_ascii_line("abc", 8, true, false, ClearMode::Exact);
    assert!(exact.contains("abc\x1b[0m   "));

    let exact_highlighted = render_ascii_line("abc", 8, true, true, ClearMode::Exact);
    assert!(exact_highlighted.contains("abc   \x1b[0m"));
}

#[test]
fn trailing_columns_use_display_width() {
    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned: Spanned<'_, UnicodeProcessor> =
        Spanned::new(&[], "界", &mut spans, &mut lines, All);
    let mut output = Vec::new();

    spanned
        .queue_print(
            &mut output,
            true,
            false,
            8,
            0,
            true,
            ClearMode::All,
            &PickerChars::new(),
        )
        .unwrap();

    assert!(String::from_utf8(output).unwrap().contains("界    \x1b[0m"));
}

#[test]
fn queue_print_line_returns_remaining_columns() {
    fn remaining_ascii(rendered: &str, capacity: u16) -> u16 {
        let mut spans = Vec::new();
        let mut lines = Vec::new();
        let spanned: Spanned<'_, AsciiProcessor> =
            Spanned::new(&[], rendered, &mut spans, &mut lines, All);
        let line = spanned.lines().next().unwrap();

        spanned
            .queue_print_line(&mut Vec::new(), line, 0, 0, capacity, &PickerChars::new())
            .unwrap()
    }

    assert_eq!(remaining_ascii("abc", 6), 3);
    assert_eq!(remaining_ascii("abcdef", 3), 0);

    let mut spans = Vec::new();
    let mut lines = Vec::new();
    let spanned: Spanned<'_, UnicodeProcessor> =
        Spanned::new(&[], "界a", &mut spans, &mut lines, All);
    let line = spanned.lines().next().unwrap();
    assert_eq!(
        spanned
            .queue_print_line(&mut Vec::new(), line, 0, 0, 6, &PickerChars::new())
            .unwrap(),
        3
    );
}
