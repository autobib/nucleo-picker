use super::{
    super::unicode::{AsciiProcessor, UnicodeProcessor, is_ascii_safe, is_unicode_safe},
    *,
};

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
