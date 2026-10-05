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

fn draw_prompt(state: &mut PromptState, width: u16, clear: ClearState) -> Vec<u8> {
    let chars = PickerChars::new();
    let mut prompt = Prompt::new(state, &chars);
    let area = Area {
        width,
        height: 1,
        ..Area::default()
    };
    prompt.resize(area, &());
    let mut output = Vec::new();
    prompt
        .draw(
            &(),
            &mut CrosstermRect::new(&mut output, area, clear).unwrap(),
        )
        .unwrap();
    output
}

#[test]
fn draw_does_not_exceed_the_available_width() {
    let mut prompt = PromptState::new(PromptConfig::new());
    let output = draw_prompt(&mut prompt, 1, ClearState::Precleared);

    assert_eq!(output, b"\x1b[1G>");
}

#[test]
fn exact_draw_fills_the_owned_width() {
    let mut prompt = PromptState::new(PromptConfig::new());
    prompt.apply(PromptEvent::Insert('a'));
    let output = draw_prompt(&mut prompt, 6, ClearState::WithinRect);

    assert_eq!(output, b"\x1b[1G      \x1b[1G> a");
}

#[test]
fn wide_grapheme_alignment_obeys_the_rectangle_clear_policy() {
    let mut prompt = init_prompt(6, 1);
    prompt.apply(PromptEvent::Paste("ＡＡＡＡＡ".to_owned()));
    assert_eq!(prompt.view(), ("ＡＡ", 1));

    for (clear, expected) in [
        (ClearState::Precleared, "\x1b[1G>  ＡＡ"),
        (ClearState::ToEndOfLine, "\x1b[1G\x1b[K>  ＡＡ"),
        (ClearState::WithinRect, "\x1b[1G        \x1b[1G>  ＡＡ"),
    ] {
        let output = draw_prompt(&mut prompt, 8, clear);
        assert_eq!(output, expected.as_bytes());
    }
}

#[test]
fn layout() {
    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Insert('a'));
    assert_eq!(editable.cursor_column(), 1);
    editable.apply(PromptEvent::Insert('Ａ'));
    assert_eq!(editable.cursor_column(), 3);
    editable.apply(PromptEvent::Insert('B'));
    assert_eq!(editable.cursor_column(), 4);

    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Paste("ＡaＡ".to_owned()));
    assert_eq!(editable.cursor_column(), 4);

    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Paste("abc".to_owned()));
    assert_eq!(editable.cursor_column(), 3);
    editable.apply(PromptEvent::Paste("ab".to_owned()));
    assert_eq!(editable.cursor_column(), 4);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 3);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 1);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 0);

    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("ＡＡＡＡＡ".to_owned()));
    editable.apply(PromptEvent::ToStart);
    assert_eq!(editable.cursor_column(), 0);
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.cursor_column(), 4);
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.cursor_column(), 5);
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.cursor_column(), 5);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 3);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.cursor_column(), 0);

    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("abc".to_owned()));
    editable.apply(PromptEvent::ToStart);
    editable.apply(PromptEvent::ToEnd);
    assert_eq!(editable.cursor_column(), 3);
    editable.apply(PromptEvent::Paste("defghi".to_owned()));
    editable.apply(PromptEvent::ToStart);
    editable.apply(PromptEvent::ToEnd);
    assert_eq!(editable.cursor_column(), 5);
}

#[test]
fn view() {
    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("abc".to_owned()));
    assert_eq!(editable.view(), ("abc", 0));

    let mut editable = init_prompt(6, 1);
    editable.apply(PromptEvent::Paste("ＡＡＡＡＡＡ".to_owned()));
    assert_eq!(editable.view(), ("ＡＡ", 1));

    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("ＡＡＡＡ".to_owned()));
    assert_eq!(editable.view(), ("ＡＡ", 1));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("ＡＡ", 1));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("ＡＡＡ", 0));

    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("012345678".to_owned()));
    editable.apply(PromptEvent::ToStart);
    assert_eq!(editable.view(), ("0123456", 0));

    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("012345Ａ".to_owned()));
    editable.apply(PromptEvent::ToStart);
    assert_eq!(editable.view(), ("012345", 0));

    let mut editable = init_prompt(4, 1);
    editable.apply(PromptEvent::Paste("01234567".to_owned()));
    assert_eq!(editable.view(), ("567", 0));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("567", 0));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("567", 0));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("4567", 0));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("3456", 0));
    editable.apply(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.apply(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("3456", 0));
}

#[test]
fn view_at_column_zero_fits_without_leading_padding() {
    for (width, padding, query, steps, expected) in [
        (1, 2, "abc", 1, "c"),
        (3, 0, "abcdef", 3, "def"),
        (2, 0, "ＡＡＡ", 1, "Ａ"),
        (1, 2, "Ａb", 1, "b"),
        (1, 2, "aＡ", 1, ""),
    ] {
        let mut prompt = init_prompt(width, padding);
        prompt.set_query(query);
        prompt.apply(PromptEvent::Left(steps));
        assert_eq!(prompt.cursor_column(), 0);

        let (contents, shift) = prompt.view();
        assert!(contents.width() + usize::from(shift) <= usize::from(width));
        assert_eq!((contents, shift), (expected, 0));
    }
}

#[test]
fn view_aligns_after_a_partially_hidden_grapheme() {
    for (width, expected) in [
        (3, ("abc", 0)),
        (4, ("abc", 1)),
        (5, ("abc", 2)),
        (6, ("\u{17d8}abc", 0)),
    ] {
        let mut prompt = init_prompt(width, 0);
        prompt.set_query("\u{17d8}abc");
        prompt.apply(PromptEvent::Left(3));
        assert_eq!(prompt.cursor_column(), width - 3);
        assert_eq!(prompt.view(), expected);

        let (contents, shift) = prompt.view();
        assert!(contents.width() + usize::from(shift) <= usize::from(width));
    }
}

#[test]
fn resize_restores_cursor_after_zero_width() {
    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Paste("abcdef".to_owned()));
    assert_eq!(editable.cursor_column(), 4);

    editable.resize(0);
    assert_eq!(editable.cursor_column(), 0);
    assert_eq!(editable.view(), ("", 0));

    editable.resize(6);
    assert_eq!(editable.cursor_column(), 4);
    assert_eq!(editable.view(), ("cdef", 0));

    editable.apply(PromptEvent::ToStart);
    editable.apply(PromptEvent::Right(2));
    editable.resize(0);
    editable.resize(6);
    assert_eq!(editable.cursor_column(), 2);
    assert_eq!(editable.view(), ("abcdef", 0));
}

#[test]
fn resize_reveals_newly_available_context() {
    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Paste("abcdefghijkl".to_owned()));
    assert_eq!(editable.cursor_column(), 4);
    assert_eq!(editable.view(), ("ijkl", 0));

    editable.resize(10);
    assert_eq!(editable.cursor_column(), 8);
    assert_eq!(editable.view(), ("efghijkl", 0));

    editable.resize(6);
    assert_eq!(editable.cursor_column(), 4);
    assert_eq!(editable.view(), ("ijkl", 0));
}

#[test]
fn resize_preserves_valid_offset_with_context_on_both_sides() {
    let mut editable = init_prompt(10, 2);
    editable.apply(PromptEvent::Paste("abcdefghijklmnop".to_owned()));
    editable.apply(PromptEvent::Left(10));
    assert_eq!(editable.cursor_column(), 2);

    editable.resize(14);
    assert_eq!(editable.cursor_column(), 2);
    assert_eq!(editable.view(), ("efghijklmnop", 0));

    editable.resize(8);
    assert_eq!(editable.cursor_column(), 2);
    assert_eq!(editable.view(), ("efghijkl", 0));
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
fn test_clear() {
    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("Ａbcde".to_owned()));
    editable.apply(PromptEvent::ToStart);
    editable.apply(PromptEvent::Right(1));
    editable.apply(PromptEvent::Right(1));
    editable.apply(PromptEvent::ClearAfter);
    assert_eq!(editable.contents(), "Ａb");
    editable.apply(PromptEvent::Insert('c'));
    editable.apply(PromptEvent::Left(1));
    editable.apply(PromptEvent::ClearBefore);
    assert_eq!(editable.contents(), "c");
}

#[test]
fn test_delete() {
    let mut editable = init_prompt(7, 2);
    editable.apply(PromptEvent::Paste("Ａb".to_owned()));
    editable.apply(PromptEvent::Backspace(1));
    assert_eq!(editable.contents(), "Ａ");
    assert_eq!(editable.cursor_column(), 2);
    editable.apply(PromptEvent::Backspace(1));
    assert_eq!(editable.contents(), "");
    assert_eq!(editable.cursor_column(), 0);
}

#[test]
fn test_normalize_prompt() {
    let mut s = "a\nb".to_owned();
    normalize_prompt_string(&mut s);
    assert_eq!(s, "a b");

    let mut s = "ｏ\nｏ".to_owned();
    normalize_prompt_string(&mut s);
    assert_eq!(s, "ｏ ｏ");

    let mut s = "a\n\u{07}ｏ".to_owned();
    normalize_prompt_string(&mut s);
    assert_eq!(s, "a ｏ");
}

#[test]
fn test_editable() {
    let mut editable = init_prompt(3, 1);
    for e in [
        PromptEvent::Insert('a'),
        PromptEvent::Left(1),
        PromptEvent::Insert('b'),
        PromptEvent::ToEnd,
        PromptEvent::Insert('c'),
        PromptEvent::ToStart,
        PromptEvent::Insert('d'),
        PromptEvent::Left(1),
        PromptEvent::Left(1),
        PromptEvent::Right(1),
        PromptEvent::Insert('e'),
    ] {
        editable.apply(e);
    }
    assert_eq!(editable.contents(), "debac");

    let mut editable = init_prompt(3, 1);
    for e in [
        PromptEvent::Insert('a'),
        PromptEvent::Insert('b'),
        PromptEvent::Insert('c'),
        PromptEvent::Insert('d'),
        PromptEvent::Left(1),
        PromptEvent::Insert('1'),
        PromptEvent::Insert('2'),
        PromptEvent::Insert('3'),
        PromptEvent::ToStart,
        PromptEvent::Backspace(1),
        PromptEvent::Insert('4'),
        PromptEvent::ToEnd,
        PromptEvent::Backspace(1),
        PromptEvent::Left(1),
        PromptEvent::Delete(1),
    ] {
        editable.apply(e);
    }

    assert_eq!(editable.contents(), "4abc12");
}

#[test]
fn test_editable_unicode() {
    let mut editable = init_prompt(3, 1);
    for e in [
        PromptEvent::Paste("दे".to_owned()),
        PromptEvent::Left(1),
        PromptEvent::Insert('a'),
        PromptEvent::ToEnd,
        PromptEvent::Insert('Ａ'),
    ] {
        editable.apply(e);
    }
    assert_eq!(editable.contents(), "aदेＡ");

    for e in [
        PromptEvent::ToStart,
        PromptEvent::Right(1),
        PromptEvent::ToEnd,
        PromptEvent::Left(1),
        PromptEvent::Backspace(1),
    ] {
        editable.apply(e);
    }

    assert_eq!(editable.contents(), "aＡ");
}

#[test]
fn sequential_emoji_insertion() {
    let mut editable = init_prompt(20, 2);
    for ch in "👩🏽‍💻".chars() {
        editable.apply(PromptEvent::Insert(ch));
    }

    assert_eq!(editable.contents(), "👩🏽‍💻");
    assert_eq!(editable.cursor_column(), 2);

    let mut pasted = init_prompt(20, 2);
    pasted.apply(PromptEvent::Paste("👩🏽‍💻".to_owned()));
    assert_eq!(editable.contents(), pasted.contents());
    assert_eq!(editable.cursor_column(), pasted.cursor_column());
    assert_eq!(editable.view(), pasted.view());

    let mut editable = init_prompt(20, 2);
    for part in ["👩", "🏽", "\u{200d}", "💻"] {
        editable.apply(PromptEvent::Paste(part.to_owned()));
    }

    assert_eq!(editable.contents(), "👩🏽‍💻");
    assert_eq!(editable.cursor_column(), 2);

    let mut editable = init_prompt(6, 2);
    editable.apply(PromptEvent::Paste("abc".to_owned()));
    for ch in "👩🏽‍💻".chars() {
        editable.apply(PromptEvent::Insert(ch));
    }

    let mut pasted = init_prompt(6, 2);
    pasted.apply(PromptEvent::Paste("abc👩🏽‍💻".to_owned()));
    assert_eq!(editable.cursor_column(), 4);
    assert_eq!(editable.cursor_column(), pasted.cursor_column());
    assert_eq!(editable.view(), pasted.view());
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
