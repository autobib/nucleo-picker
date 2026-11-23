use super::{Prompt, PromptConfig, PromptState};
use crate::{
    PickerChars,
    component::{Component, tests::TestSurface},
    event::PromptEvent::{self, *},
    rect::{Dimensions, Position},
};

fn render(prompt: &mut Prompt<'_>, width: u16) -> String {
    prompt.update();
    let mut target = TestSurface::new(width + 2, 1);
    prompt.draw(&mut target).unwrap();
    target.rows.pop().unwrap()
}

fn assert_prompt(prompt: &mut Prompt<'_>, width: u16, query: &str, column: u16, visible: &str) {
    assert_eq!(render(prompt, width), format!("> {visible}"));
    assert_eq!(
        prompt.cursor(),
        Some(Position {
            column: (column + 2).min(width + 1),
            row: 0,
        })
    );
    assert_eq!(prompt.contents(), query);
}

#[test]
fn cursor_and_visible_text_scroll_across_grapheme_boundaries() {
    for (width, padding, query, steps) in [
        (
            4,
            1,
            "01234567",
            [
                (ToEnd, 3, "567"),
                (Left(3), 1, "4567"),
                (Left(2), 1, "2345"),
                (Right(3), 3, "3456"),
                (ToStart, 0, "0123"),
            ],
        ),
        (
            7,
            2,
            "ＡＡＡＡＡ",
            [
                (ToEnd, 5, " ＡＡ"),
                (Left(2), 2, "ＡＡＡ"),
                (Left(3), 0, "ＡＡＡ"),
                (Right(3), 5, " ＡＡＡ"),
                (ToStart, 0, "ＡＡＡ"),
            ],
        ),
    ] {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig { padding });
        let mut prompt = Prompt::new(&mut state, &chars);
        render(&mut prompt, width);
        prompt.handle(Paste(query.to_owned()));
        for (event, column, visible) in steps {
            prompt.handle(event);
            assert_prompt(&mut prompt, width, query, column, visible);
        }
    }
}

#[test]
fn narrow_prompts_clip_wide_graphemes_without_splitting_them() {
    for (width, query, left, column, visible) in [
        (1, "abc", 1, 0, "c"),
        (1, "aＡ", 1, 0, ""),
        (3, "\u{17d8}abc", 3, 0, "abc"),
        (4, "\u{17d8}abc", 3, 1, " abc"),
        (6, "\u{17d8}abc", 3, 3, "\u{17d8}abc"),
    ] {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig { padding: 0 });
        let mut prompt = Prompt::new(&mut state, &chars);
        render(&mut prompt, width);
        prompt.handle(Paste(query.to_owned()));
        prompt.handle(Left(left));
        assert_prompt(&mut prompt, width, query, column, visible);
    }
}

#[test]
fn resizing_restores_cursor_and_context_after_zero_width() {
    let chars = PickerChars::new();
    let mut state = PromptState::new(PromptConfig { padding: 2 });
    let mut prompt = Prompt::new(&mut state, &chars);
    render(&mut prompt, 6);
    prompt.handle(Paste("abcdefghijkl".to_owned()));
    for (width, column, visible) in [
        (6, 4, "ijkl"),
        (0, 0, ""),
        (10, 8, "efghijkl"),
        (6, 4, "ijkl"),
    ] {
        assert_prompt(&mut prompt, width, "abcdefghijkl", column, visible);
    }
}

#[test]
fn edits_preserve_graphemes_and_clear_at_byte_boundaries() {
    let chars = PickerChars::new();
    let mut state = PromptState::new(PromptConfig { padding: 1 });
    let mut prompt = Prompt::new(&mut state, &chars);
    render(&mut prompt, 3);
    for (event, query, column, visible) in [
        (Paste("दे".to_owned()), "दे", 1, "दे"),
        (Left(1), "दे", 0, "दे"),
        (Insert('a'), "aदे", 1, "aदे"),
        (ToEnd, "aदे", 2, "aदे"),
        (Insert('Ａ'), "aदेＡ", 2, "Ａ"),
        (Left(1), "aदेＡ", 1, "देＡ"),
        (Backspace(1), "aＡ", 1, "aＡ"),
        (ToEnd, "aＡ", 2, "Ａ"),
        (Insert('b'), "aＡb", 2, " b"),
        (Left(1), "aＡb", 1, " b"),
        (ClearAfter, "aＡ", 1, " "),
        (Insert('c'), "aＡc", 2, " c"),
        (Left(1), "aＡc", 1, " c"),
        (ClearBefore, "c", 0, "c"),
        (Delete(1), "", 0, ""),
    ] {
        prompt.handle(event);
        assert_prompt(&mut prompt, 3, query, column, visible);
    }
}

#[test]
fn insertion_before_variation_selector_keeps_rendering_within_width() {
    for (event, prefix, column) in [
        (Insert('❤'), "", 2),
        (Paste("❤".to_owned()), "", 2),
        (Paste("ab❤".to_owned()), "ab", 4),
    ] {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig::default());
        let mut prompt = Prompt::new(&mut state, &chars);
        render(&mut prompt, 78);
        prompt.handle(Reset(format!("\u{fe0f}{}", "a".repeat(80))));
        prompt.handle(ToStart);
        prompt.handle(event);
        assert_prompt(
            &mut prompt,
            78,
            &format!("{prefix}❤️{}", "a".repeat(80)),
            column,
            &format!("{prefix}❤️{}", "a".repeat(78 - usize::from(column))),
        );
    }
}

#[test]
fn inserting_zwj_keeps_combined_emoji_whole_during_deletion() {
    for delete_from_start in [false, true] {
        for event in [Insert('\u{200d}'), Paste("\u{200d}".to_owned())] {
            let chars = PickerChars::new();
            let mut state = PromptState::new(PromptConfig::default());
            let mut prompt = Prompt::new(&mut state, &chars);
            prompt.handle(Reset("👩💻x".to_owned()));
            prompt.handle(ToStart);
            prompt.handle(Right(1));
            prompt.handle(event);
            assert_prompt(&mut prompt, 8, "👩‍💻x", 2, "👩‍💻x");
            if delete_from_start {
                prompt.handle(Left(1));
                prompt.handle(Delete(1));
            } else {
                prompt.handle(Backspace(1));
            }
            assert_prompt(&mut prompt, 8, "x", 0, "x");
        }
    }
}

#[test]
fn insertion_positions_following_edits_at_complete_grapheme_boundaries() {
    for (initial, right, ch, query, column, edited) in [
        ("\u{301}x", 0, 'e', "e\u{301}x", 1, "e\u{301}!x"),
        ("ex", 1, '\u{301}', "e\u{301}x", 1, "e\u{301}!x"),
        ("ab", 1, 'X', "aXb", 2, "aX!b"),
        ("ab", 0, 'X', "Xab", 1, "X!ab"),
        ("", 0, 'x', "x", 1, "x!"),
        ("🇦🇧🇨🇩x", 1, '🇪', "🇦🇧🇪🇨🇩x", 4, "🇦🇧🇪🇨!🇩x"),
    ] {
        for event in [Insert(ch), Paste(ch.to_string())] {
            let chars = PickerChars::new();
            let mut state = PromptState::new(PromptConfig::default());
            let mut prompt = Prompt::new(&mut state, &chars);
            prompt.handle(Reset(initial.to_owned()));
            prompt.handle(ToStart);
            prompt.handle(Right(right));
            prompt.handle(event);
            assert_prompt(&mut prompt, 12, query, column, query);
            prompt.handle(Insert('!'));
            assert_prompt(&mut prompt, 12, edited, column + 1, edited);
        }
    }
}

#[test]
fn word_edits_and_zero_count_operations_preserve_insertion_boundaries() {
    for (query, steps, moved, deleted) in [
        ("   ", 1, "X   ", ""),
        ("...", 1, "X...", ""),
        ("  one two", 1, "  one Xtwo", "  one "),
        ("  one two", usize::MAX, "  Xone two", "  "),
    ] {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig::default());
        let mut prompt = Prompt::new(&mut state, &chars);
        prompt.handle(Reset(query.to_owned()));
        prompt.handle(WordLeft(steps));
        prompt.handle(Insert('X'));
        prompt.update();
        assert_eq!(prompt.contents(), moved);
        prompt.handle(Reset(query.to_owned()));
        prompt.handle(BackspaceWord(steps));
        prompt.update();
        assert_eq!(prompt.contents(), deleted);
    }

    for event in [
        Left as fn(usize) -> PromptEvent,
        WordLeft,
        Backspace,
        BackspaceWord,
    ] {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig::default());
        let mut prompt = Prompt::new(&mut state, &chars);
        prompt.handle(Paste("Ａe\u{301}界".to_owned()));
        prompt.handle(Left(1));
        let before = render(&mut prompt, 3);
        prompt.handle(event(0));
        assert_eq!(render(&mut prompt, 3), before);
        prompt.handle(Insert('X'));
        prompt.update();
        assert_eq!(prompt.contents(), "Ａe\u{301}X界");
    }
}

#[test]
fn incremental_input_matches_paste_for_emoji_and_control_characters() {
    for (text, normalized) in [("abc👩🏽‍💻", "abc👩🏽‍💻"), ("a\n\u{7}ｏ\tb", "a ｏ b")]
    {
        let chars = PickerChars::new();
        let mut state = PromptState::new(PromptConfig::default());
        let mut prompt = Prompt::new(&mut state, &chars);
        render(&mut prompt, 6);
        prompt.handle(Paste(text.to_owned()));
        let pasted = render(&mut prompt, 6);
        let cursor = prompt.cursor();
        assert_eq!(prompt.contents(), normalized);
        prompt.handle(Reset(String::new()));
        for ch in text.chars() {
            prompt.handle(Insert(ch));
        }
        assert_eq!(render(&mut prompt, 6), pasted);
        assert_eq!(prompt.contents(), normalized);
        assert_eq!(prompt.cursor(), cursor);
    }
}

#[test]
fn target_dimensions_control_empty_drawing_and_local_cursor() {
    let chars = PickerChars::new();
    let mut state = PromptState::new(PromptConfig::default());
    let mut prompt = Prompt::new(&mut state, &chars);
    assert_eq!(prompt.cursor(), None);
    assert_eq!(prompt.view(), ("", 0));

    for (width, height, cursor) in [
        (8, 1, Some(2)),
        (0, 1, None),
        (8, 0, None),
        (1, 1, Some(0)),
        (2, 1, Some(1)),
        (8, 3, Some(2)),
    ] {
        let mut target = TestSurface::new(width, height);
        prompt.draw(&mut target).unwrap();
        assert_eq!(target.touched, width != 0 && height != 0);
        assert_eq!(
            prompt.cursor(),
            cursor.map(|column| Position { column, row: 0 })
        );
        prompt.resize(Dimensions { width, height });
    }
    prompt.handle(Paste("text".to_owned()));
    prompt.update();
    prompt.resize(Dimensions {
        width: 8,
        height: 0,
    });
    assert_eq!(prompt.view(), ("", 0));
    prompt.resize(Dimensions {
        width: 8,
        height: 1,
    });
    assert_eq!(prompt.view(), ("text", 0));
}
