use super::{state::normalize_prompt_string, *};
use crate::rect::{Area, ClearState, CrosstermRect};
use crate::{PickerChars, component::Component};
use unicode_width::UnicodeWidthStr;

fn init_prompt(width: u16, padding: u16) -> PromptState {
    let cfg = PromptConfig { padding };
    let mut prompt = PromptState::new(cfg);
    prompt.resize(width);
    prompt
}

fn assert_state(prompt: &PromptState, expected: (&str, u16, (&str, u16))) {
    assert_eq!(
        (prompt.contents(), prompt.cursor_column(), prompt.view()),
        expected,
    );
}

fn draw_prompt(state: &mut PromptState, width: u16, clear: ClearState) -> Vec<u8> {
    let chars = PickerChars::new();
    let mut prompt = Prompt::new(state, &chars);
    let area = Area {
        width,
        height: 1,
        ..Area::default()
    };
    prompt.resize(area);
    let mut output = Vec::new();
    prompt
        .draw(&mut CrosstermRect::new(&mut output, area, clear).unwrap())
        .unwrap();
    output
}

#[test]
fn drawing_respects_width_grapheme_alignment_and_clear_policy() {
    use ClearState::{Precleared, ToEndOfLine, WithinRect};

    for (width, padding, query, clear, expected) in [
        (1, 2, "", Precleared, "\x1b[1G>"),
        (6, 2, "a", WithinRect, "\x1b[1G      \x1b[1G> a"),
        (8, 1, "ＡＡＡＡＡ", Precleared, "\x1b[1G>  ＡＡ"),
        (8, 1, "ＡＡＡＡＡ", ToEndOfLine, "\x1b[1G\x1b[K>  ＡＡ"),
        (
            8,
            1,
            "ＡＡＡＡＡ",
            WithinRect,
            "\x1b[1G        \x1b[1G>  ＡＡ",
        ),
    ] {
        let mut prompt = init_prompt(width, padding);
        prompt.apply(PromptEvent::Paste(query.to_owned()));
        let output = draw_prompt(&mut prompt, width, clear);
        assert_eq!(output, expected.as_bytes());
    }
}

#[test]
fn cursor_and_view_scroll_across_grapheme_boundaries() {
    use PromptEvent::*;

    for (width, padding, query, steps) in [
        (
            4,
            1,
            "01234567",
            [
                (ToEnd, 3, ("567", 0)),
                (Left(1), 2, ("567", 0)),
                (Left(1), 1, ("567", 0)),
                (Left(1), 1, ("4567", 0)),
                (Left(1), 1, ("3456", 0)),
                (Left(1), 1, ("2345", 0)),
                (Right(1), 2, ("2345", 0)),
                (Right(1), 3, ("2345", 0)),
                (Right(1), 3, ("3456", 0)),
                (ToStart, 0, ("0123", 0)),
                (ToEnd, 3, ("567", 0)),
            ],
        ),
        (
            7,
            2,
            "ＡＡＡＡＡ",
            [
                (ToEnd, 5, ("ＡＡ", 1)),
                (Left(1), 3, ("ＡＡ", 1)),
                (Left(1), 2, ("ＡＡＡ", 0)),
                (Left(1), 2, ("ＡＡＡ", 0)),
                (Left(1), 2, ("ＡＡＡ", 0)),
                (Left(1), 0, ("ＡＡＡ", 0)),
                (Right(1), 2, ("ＡＡＡ", 0)),
                (Right(1), 4, ("ＡＡＡ", 0)),
                (Right(1), 5, ("ＡＡＡ", 1)),
                (ToStart, 0, ("ＡＡＡ", 0)),
                (ToEnd, 5, ("ＡＡ", 1)),
            ],
        ),
    ] {
        let mut prompt = init_prompt(width, padding);
        prompt.apply(Paste(query.to_owned()));
        for (event, column, view) in steps {
            prompt.apply(event);
            assert_state(&prompt, (query, column, view));
        }
    }
}

#[test]
fn view_fits_at_column_zero_and_after_partially_hidden_graphemes() {
    for (width, padding, query, steps, column, expected) in [
        (1, 2, "abc", 1, 0, ("c", 0)),
        (3, 0, "abcdef", 3, 0, ("def", 0)),
        (2, 0, "ＡＡＡ", 1, 0, ("Ａ", 0)),
        (1, 2, "Ａb", 1, 0, ("b", 0)),
        (1, 2, "aＡ", 1, 0, ("", 0)),
        (3, 0, "\u{17d8}abc", 3, 0, ("abc", 0)),
        (4, 0, "\u{17d8}abc", 3, 1, ("abc", 1)),
        (5, 0, "\u{17d8}abc", 3, 2, ("abc", 2)),
        (6, 0, "\u{17d8}abc", 3, 3, ("\u{17d8}abc", 0)),
        (7, 2, "012345Ａ", usize::MAX, 0, ("012345", 0)),
        (7, 2, "012345678", usize::MAX, 0, ("0123456", 0)),
        (7, 2, "abc", 0, 3, ("abc", 0)),
    ] {
        let mut prompt = init_prompt(width, padding);
        prompt.set_query(query);
        prompt.apply(PromptEvent::Left(steps));
        assert_state(&prompt, (query, column, expected));
        let (contents, shift) = prompt.view();
        assert!(contents.width() + usize::from(shift) <= usize::from(width));
    }
}

#[test]
fn resizing_restores_the_cursor_and_reveals_available_context() {
    for (width, query, left, expected) in [
        (6, "abcdef", 0, [(6, 4, "cdef"), (0, 0, ""), (6, 4, "cdef")]),
        (
            6,
            "abcdef",
            4,
            [(6, 2, "abcdef"), (0, 0, ""), (6, 2, "abcdef")],
        ),
        (
            6,
            "abcdefghijkl",
            0,
            [(6, 4, "ijkl"), (10, 8, "efghijkl"), (6, 4, "ijkl")],
        ),
        (
            10,
            "abcdefghijklmnop",
            10,
            [
                (10, 2, "efghijklmn"),
                (14, 2, "efghijklmnop"),
                (8, 2, "efghijkl"),
            ],
        ),
    ] {
        let mut prompt = init_prompt(width, 2);
        prompt.apply(PromptEvent::Paste(query.to_owned()));
        prompt.apply(PromptEvent::Left(left));
        for (width, column, view) in expected {
            prompt.resize(width);
            assert_state(&prompt, (query, column, (view, 0)));
        }
    }
}

#[test]
fn test_word_movement() {
    let mut editable = init_prompt(100, 2);
    editable.apply(PromptEvent::Paste("one two".to_owned()));
    editable.apply(PromptEvent::WordLeft(1));
    editable.apply(PromptEvent::WordLeft(1));
    assert_eq!(editable.cursor_column(), 0);
    editable.apply(PromptEvent::WordRight(1));
    assert_eq!(editable.cursor_column(), 4);
    editable.apply(PromptEvent::WordRight(1));
    assert_eq!(editable.cursor_column(), 7);
    editable.apply(PromptEvent::WordRight(1));
    assert_eq!(editable.cursor_column(), 7);
}

fn assert_zero_back_is_noop(event: fn(usize) -> PromptEvent) {
    for width in [3, 20] {
        for (query, steps, after_insertion) in [
            ("", 0, "X"),
            ("abc", 0, "Xabc"),
            ("abc", 1, "aXbc"),
            ("abc", 3, "abcX"),
            ("Ａe\u{301}界", 0, "XＡe\u{301}界"),
            ("Ａe\u{301}界", 2, "Ａe\u{301}X界"),
            ("Ａe\u{301}界", 3, "Ａe\u{301}界X"),
            ("   ", 2, "  X "),
            ("...", 3, "...X"),
        ] {
            let mut prompt = init_prompt(width, 1);
            prompt.set_query(query);
            prompt.apply(PromptEvent::ToStart);
            prompt.apply(PromptEvent::Right(steps));
            let column = prompt.cursor_column();
            let (contents, shift) = prompt.view();
            let view = (contents.to_owned(), shift);

            let status = prompt.apply(event(0));

            assert!(!status.needs_redraw);
            assert!(!status.contents_changed);
            assert_eq!(prompt.contents(), query);
            assert_eq!(prompt.cursor_column(), column);
            assert_eq!(prompt.view(), (view.0.as_str(), view.1));

            prompt.apply(PromptEvent::Insert('X'));
            assert_eq!(prompt.contents(), after_insertion);
        }
    }
}

#[test]
fn zero_count_left_is_noop() {
    assert_zero_back_is_noop(PromptEvent::Left);
    assert_zero_back_is_noop(PromptEvent::WordLeft);
    assert_zero_back_is_noop(PromptEvent::Backspace);
    assert_zero_back_is_noop(PromptEvent::BackspaceWord);
}

#[test]
fn backward_word_operations_preserve_boundaries() {
    for (query, steps, after_insertion, after_deletion) in [
        ("   ", 1, "X   ", ""),
        ("...", 1, "X...", ""),
        ("  one two", 1, "  one Xtwo", "  one "),
        ("  one two", 2, "  Xone two", "  "),
        ("  one two", usize::MAX, "  Xone two", "  "),
    ] {
        let mut prompt = init_prompt(20, 1);
        prompt.set_query(query);
        prompt.apply(PromptEvent::WordLeft(steps));
        prompt.apply(PromptEvent::Insert('X'));
        assert_eq!(prompt.contents(), after_insertion);

        prompt.set_query(query);
        prompt.apply(PromptEvent::BackspaceWord(steps));
        assert_eq!(prompt.contents(), after_deletion);
    }
}

#[test]
fn test_normalize_prompt() {
    for (input, expected) in [
        ("a\nb", "a b"),
        ("ｏ\nｏ", "ｏ ｏ"),
        ("a\n\u{07}ｏ", "a ｏ"),
    ] {
        let mut s = input.to_owned();
        normalize_prompt_string(&mut s);
        assert_eq!(s, expected);
    }
}

#[test]
fn editing_preserves_the_cursor_and_scrolled_context() {
    use PromptEvent::*;

    let mut prompt = init_prompt(3, 1);
    for (event, contents, column, view) in [
        (Insert('a'), "a", 1, "a"),
        (Insert('b'), "ab", 2, "ab"),
        (Insert('c'), "abc", 2, "bc"),
        (Insert('d'), "abcd", 2, "cd"),
        (Left(1), "abcd", 1, "cd"),
        (Insert('1'), "abc1d", 2, "c1d"),
        (Insert('2'), "abc12d", 2, "12d"),
        (Insert('3'), "abc123d", 2, "23d"),
        (ToStart, "abc123d", 0, "abc"),
        (Backspace(1), "abc123d", 0, "abc"),
        (Insert('4'), "4abc123d", 1, "4ab"),
        (ToEnd, "4abc123d", 2, "3d"),
        (Backspace(1), "4abc123", 1, "3"),
        (Left(1), "4abc123", 1, "23"),
        (Delete(1), "4abc12", 1, "2"),
        (ToStart, "4abc12", 0, "4ab"),
        (Right(2), "4abc12", 2, "4ab"),
        (ClearAfter, "4a", 2, "4a"),
        (Insert('b'), "4ab", 2, "ab"),
        (Left(1), "4ab", 1, "ab"),
        (ClearBefore, "b", 0, "b"),
        (Delete(1), "", 0, ""),
    ] {
        prompt.apply(event);
        assert_state(&prompt, (contents, column, (view, 0)));
    }
}

#[test]
fn unicode_edits_preserve_graphemes_and_clear_at_byte_boundaries() {
    use PromptEvent::*;

    let mut prompt = init_prompt(3, 1);
    for (event, contents, column, view) in [
        (Paste("दे".to_owned()), "दे", 1, ("दे", 0)),
        (Left(1), "दे", 0, ("दे", 0)),
        (Insert('a'), "aदे", 1, ("aदे", 0)),
        (ToEnd, "aदे", 2, ("aदे", 0)),
        (Insert('Ａ'), "aदेＡ", 2, ("Ａ", 0)),
        (ToStart, "aदेＡ", 0, ("aदे", 0)),
        (Right(1), "aदेＡ", 1, ("aदे", 0)),
        (ToEnd, "aदेＡ", 2, ("Ａ", 0)),
        (Left(1), "aदेＡ", 1, ("देＡ", 0)),
        (Backspace(1), "aＡ", 1, ("aＡ", 0)),
        (ToEnd, "aＡ", 2, ("Ａ", 0)),
        (Insert('b'), "aＡb", 2, ("b", 1)),
        (Left(1), "aＡb", 1, ("b", 1)),
        (ClearAfter, "aＡ", 1, ("", 1)),
        (Insert('c'), "aＡc", 2, ("c", 1)),
        (Left(1), "aＡc", 1, ("c", 1)),
        (ClearBefore, "c", 0, ("c", 0)),
        (Insert('Ａ'), "Ａc", 2, ("Ａc", 0)),
        (Backspace(1), "c", 0, ("c", 0)),
        (Delete(1), "", 0, ("", 0)),
    ] {
        prompt.apply(event);
        assert_state(&prompt, (contents, column, view));
    }
}

#[test]
fn sequential_emoji_insertion() {
    for (width, prefix, column) in [(20, "", 2), (6, "abc", 4)] {
        let contents = format!("{prefix}👩🏽‍💻");
        let mut pasted = init_prompt(width, 2);
        pasted.apply(PromptEvent::Paste(contents.clone()));
        assert_eq!(pasted.cursor_column(), column);
        for paste_parts in [false, true] {
            let mut prompt = init_prompt(width, 2);
            prompt.apply(PromptEvent::Paste(prefix.to_owned()));
            for ch in "👩🏽‍💻".chars() {
                prompt.apply(if paste_parts {
                    PromptEvent::Paste(ch.to_string())
                } else {
                    PromptEvent::Insert(ch)
                });
            }
            assert_state(&prompt, (&contents, column, pasted.view()));
        }
    }
}

#[test]
fn buffered_changes_distinguish_cursor_movement_from_edits() {
    let mut state = init_prompt(10, 2);
    state.set_query("abc");
    let chars = PickerChars::new();
    let mut prompt = Prompt::new(&mut state, &chars);

    prompt.handle(PromptEvent::Left(1));
    prompt.handle(PromptEvent::Left(1));
    let status = prompt.update();
    assert!(status.needs_redraw);
    assert!(!status.contents_changed);

    prompt.handle(PromptEvent::Insert('x'));
    prompt.handle(PromptEvent::Right(1));
    prompt.flush();
    assert_eq!(prompt.contents(), "axbc");
    let status = prompt.update();
    assert!(status.needs_redraw);
    assert!(status.contents_changed);

    let status = prompt.update();
    assert!(!status.needs_redraw);
    assert!(!status.contents_changed);

    prompt.handle(PromptEvent::Insert('\u{7}'));
    let status = prompt.update();
    assert!(!status.needs_redraw);
    assert!(!status.contents_changed);
}
