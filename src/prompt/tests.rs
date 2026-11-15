use std::time::Duration;

use crate::{
    DriverOptions,
    event::PromptEvent::{self, *},
    rect::Area,
    render::StrRenderer,
    session::{
        Dimensions, Geometry, Redraw, Session,
        tests::{Select, TestTerminal, frame_options},
    },
};

fn render(session: &mut Session<'_, &'static str, StrRenderer, Select>, width: u16) -> String {
    let mut options = frame_options();
    options.visible_components = Redraw {
        prompt: true,
        ..Redraw::NONE
    };
    let mut frame = session.prepare_frame(options);
    frame.resize(Geometry {
        dimensions: Dimensions {
            width: width + 2,
            height: 1,
        },
        prompt: Area {
            width: width + 2,
            height: 1,
            ..Area::default()
        },
        ..Geometry::default()
    });
    let mut terminal = TestTerminal::default();
    frame.draw(&mut terminal, Redraw::FULL).unwrap();
    frame.complete();
    String::from_utf8(terminal.output).unwrap()
}

fn assert_prompt(
    session: &mut Session<'_, &'static str, StrRenderer, Select>,
    width: u16,
    query: &str,
    column: u16,
    visible: &str,
) {
    let output = render(session, width);
    assert!(output.contains(&format!("> {visible}\x1b")), "{output:?}");
    let column = (column + 2).min(width + 1) + 1;
    assert!(
        output.ends_with(&format!("\x1b[1;{column}H\x1b[?2026l")),
        "{output:?}"
    );
    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.view().query(), query);
    frame.complete();
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
        let mut driver = DriverOptions::new()
            .prompt_padding(padding)
            .driver(StrRenderer);
        let mut session = driver.session(Select, Duration::ZERO);
        render(&mut session, width);
        session.events().prompt(Paste(query.to_owned()));
        for (event, column, visible) in steps {
            session.events().prompt(event);
            assert_prompt(&mut session, width, query, column, visible);
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
        let mut driver = DriverOptions::new().prompt_padding(0).driver(StrRenderer);
        let mut session = driver.session(Select, Duration::ZERO);
        render(&mut session, width);
        {
            let mut batch = session.events();
            batch.prompt(Paste(query.to_owned()));
            batch.prompt(Left(left));
        }
        assert_prompt(&mut session, width, query, column, visible);
    }
}

#[test]
fn resizing_restores_cursor_and_context_after_zero_width() {
    let mut driver = DriverOptions::new().prompt_padding(2).driver(StrRenderer);
    let mut session = driver.session(Select, Duration::ZERO);
    render(&mut session, 6);
    session.events().prompt(Paste("abcdefghijkl".to_owned()));
    for (width, column, visible) in [
        (6, 4, "ijkl"),
        (0, 0, ""),
        (10, 8, "efghijkl"),
        (6, 4, "ijkl"),
    ] {
        assert_prompt(&mut session, width, "abcdefghijkl", column, visible);
    }
}

#[test]
fn edits_preserve_graphemes_and_clear_at_byte_boundaries() {
    let mut driver = DriverOptions::new().prompt_padding(1).driver(StrRenderer);
    let mut session = driver.session(Select, Duration::ZERO);
    render(&mut session, 3);
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
        session.events().prompt(event);
        assert_prompt(&mut session, 3, query, column, visible);
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
        let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
        let mut session = driver.session(Select, Duration::ZERO);
        {
            let mut batch = session.events();
            batch.prompt(Reset(query.to_owned()));
            batch.prompt(WordLeft(steps));
            batch.prompt(Insert('X'));
        }
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.view().query(), moved);
        frame.complete();
        {
            let mut batch = session.events();
            batch.prompt(Reset(query.to_owned()));
            batch.prompt(BackspaceWord(steps));
        }
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.view().query(), deleted);
        frame.complete();
    }

    for event in [
        Left as fn(usize) -> PromptEvent,
        WordLeft,
        Backspace,
        BackspaceWord,
    ] {
        let mut driver = DriverOptions::new().driver(StrRenderer);
        let mut session = driver.session(Select, Duration::ZERO);
        session.events().prompt(Paste("Ａe\u{301}界".to_owned()));
        session.events().prompt(Left(1));
        let before = render(&mut session, 3);
        session.events().prompt(event(0));
        assert_eq!(render(&mut session, 3), before);
        session.events().prompt(Insert('X'));
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.view().query(), "Ａe\u{301}X界");
        frame.complete();
    }
}

#[test]
fn incremental_input_matches_paste_for_emoji_and_control_characters() {
    for text in ["abc👩🏽‍💻", "a\n\u{7}ｏ\tb"] {
        let mut driver = DriverOptions::new().driver(StrRenderer);
        let mut session = driver.session(Select, Duration::ZERO);
        render(&mut session, 6);
        session.events().prompt(Paste(text.to_owned()));
        let pasted = render(&mut session, 6);
        session.events().prompt(Reset(String::new()));
        for ch in text.chars() {
            session.events().prompt(Insert(ch));
        }
        assert_eq!(render(&mut session, 6), pasted);
    }
}
