use super::*;

fn init_prompt(width: u16, padding: u16) -> Prompt {
    let mut cfg = PromptConfig::default();
    cfg.padding = padding;
    let mut prompt = Prompt::new(cfg);
    prompt.resize(width);
    prompt
}

#[test]
fn layout() {
    let mut editable = init_prompt(6, 2);
    editable.handle(PromptEvent::Insert('a'));
    assert_eq!(editable.screen_offset, 1);
    editable.handle(PromptEvent::Insert('Ａ'));
    assert_eq!(editable.screen_offset, 3);
    editable.handle(PromptEvent::Insert('B'));
    assert_eq!(editable.screen_offset, 4);

    let mut editable = init_prompt(6, 2);
    editable.handle(PromptEvent::Paste("ＡaＡ".to_owned()));
    assert_eq!(editable.screen_offset, 4);

    let mut editable = init_prompt(6, 2);
    editable.handle(PromptEvent::Paste("abc".to_owned()));
    assert_eq!(editable.screen_offset, 3);
    editable.handle(PromptEvent::Paste("ab".to_owned()));
    assert_eq!(editable.screen_offset, 4);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 3);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 1);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 0);

    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("ＡＡＡＡＡ".to_owned()));
    editable.handle(PromptEvent::ToStart);
    assert_eq!(editable.screen_offset, 0);
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.screen_offset, 4);
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.screen_offset, 5);
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.screen_offset, 5);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 3);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.screen_offset, 0);

    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("abc".to_owned()));
    editable.handle(PromptEvent::ToStart);
    editable.handle(PromptEvent::ToEnd);
    assert_eq!(editable.screen_offset, 3);
    editable.handle(PromptEvent::Paste("defghi".to_owned()));
    editable.handle(PromptEvent::ToStart);
    editable.handle(PromptEvent::ToEnd);
    assert_eq!(editable.screen_offset, 5);
}

#[test]
fn view() {
    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("abc".to_owned()));
    assert_eq!(editable.view(), ("abc", 0));

    let mut editable = init_prompt(6, 1);
    editable.handle(PromptEvent::Paste("ＡＡＡＡＡＡ".to_owned()));
    assert_eq!(editable.view(), ("ＡＡ", 1));

    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("ＡＡＡＡ".to_owned()));
    assert_eq!(editable.view(), ("ＡＡ", 1));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("ＡＡ", 1));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("ＡＡＡ", 0));

    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("012345678".to_owned()));
    editable.handle(PromptEvent::ToStart);
    assert_eq!(editable.view(), ("0123456", 0));

    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("012345Ａ".to_owned()));
    editable.handle(PromptEvent::ToStart);
    assert_eq!(editable.view(), ("012345", 0));

    let mut editable = init_prompt(4, 1);
    editable.handle(PromptEvent::Paste("01234567".to_owned()));
    assert_eq!(editable.view(), ("567", 0));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("567", 0));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("567", 0));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("4567", 0));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("3456", 0));
    editable.handle(PromptEvent::Left(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("2345", 0));
    editable.handle(PromptEvent::Right(1));
    assert_eq!(editable.view(), ("3456", 0));
}

#[test]
fn test_word_movement() {
    let mut editable = init_prompt(100, 2);
    editable.handle(PromptEvent::Paste("one two".to_owned()));
    editable.handle(PromptEvent::WordLeft(1));
    editable.handle(PromptEvent::WordLeft(1));
    assert_eq!(editable.screen_offset, 0);
    editable.handle(PromptEvent::WordRight(1));
    assert_eq!(editable.screen_offset, 4);
    editable.handle(PromptEvent::WordRight(1));
    assert_eq!(editable.screen_offset, 7);
    editable.handle(PromptEvent::WordRight(1));
    assert_eq!(editable.screen_offset, 7);
}

#[test]
fn test_clear() {
    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("Ａbcde".to_owned()));
    editable.handle(PromptEvent::ToStart);
    editable.handle(PromptEvent::Right(1));
    editable.handle(PromptEvent::Right(1));
    editable.handle(PromptEvent::ClearAfter);
    assert_eq!(editable.contents, "Ａb");
    editable.handle(PromptEvent::Insert('c'));
    editable.handle(PromptEvent::Left(1));
    editable.handle(PromptEvent::ClearBefore);
    assert_eq!(editable.contents, "c");
}

#[test]
fn test_delete() {
    let mut editable = init_prompt(7, 2);
    editable.handle(PromptEvent::Paste("Ａb".to_owned()));
    editable.handle(PromptEvent::Backspace(1));
    assert_eq!(editable.contents, "Ａ");
    assert_eq!(editable.screen_offset, 2);
    editable.handle(PromptEvent::Backspace(1));
    assert_eq!(editable.contents, "");
    assert_eq!(editable.screen_offset, 0);
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
        editable.handle(e);
    }
    assert_eq!(editable.contents, "debac");

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
        editable.handle(e);
    }

    assert_eq!(editable.contents, "4abc12");
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
        editable.handle(e);
    }
    assert_eq!(editable.contents, "aदेＡ");

    for e in [
        PromptEvent::ToStart,
        PromptEvent::Right(1),
        PromptEvent::ToEnd,
        PromptEvent::Left(1),
        PromptEvent::Backspace(1),
    ] {
        editable.handle(e);
    }

    assert_eq!(editable.contents, "aＡ");
}
