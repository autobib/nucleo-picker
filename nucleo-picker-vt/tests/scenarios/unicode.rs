use std::error::Error;

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
};

use super::{ScenarioRunner, unicode_lines};

#[test]
fn unicode_truncation_preserves_the_match_prefix() -> Result<(), Box<dyn Error>> {
    let mut sr = ScenarioRunner::start_multi_with_options(
        "unicode_truncation_preserves_the_match_prefix",
        vec!["界a"],
        PickerOptions::new(),
    );
    sr.set_dimensions(3, 3)?;
    sr.type_text("a")?;
    let selected = sr.checkpoint("selected")?;
    assert_eq!(selected.text[0], "▌ …");

    sr.send(Event::MatchList(MatchListEvent::ToggleUp(1)))?;
    let queued = sr.checkpoint("queued")?;
    assert_eq!(queued.text[0], "▌┃…");

    sr.send(Event::Select)?;
    assert_eq!(sr.finish()?, ["界a"]);
    Ok(())
}

#[test]
fn basic_unicode() -> Result<(), Box<dyn Error>> {
    let mut sr =
        ScenarioRunner::start_with_options("basic_unicode", unicode_lines(), PickerOptions::new());
    sr.set_dimensions(100, 14)?;
    checkpoint!(sr, "wide-initial");
    sr.set_dimensions(20, 12)?;
    let narrow = checkpoint!(sr, "narrow-right-elision");
    sr.set_dimensions(20, 3)?;
    checkpoint!(sr, "short");

    for (query, checkpoint) in [
        ("NFCcafé", "latin-nfc-highlight"),
        ("NFDcafe\u{301}", "latin-nfd-highlight"),
        ("한국어서울", "korean-highlight"),
        ("日本語東京", "japanese-highlight"),
        ("👨‍👩‍👧‍👦", "family-emoji-highlight"),
        ("👩🏽‍💻", "profession-emoji-highlight"),
        ("🏳️‍🌈", "flag-emoji-highlight"),
    ] {
        sr.type_text(query)?;
        checkpoint!(sr, checkpoint);
        sr.send(Event::Prompt(PromptEvent::ClearBefore))?;
    }

    sr.set_dimensions(20, 12)?;
    assert_eq!(sr.checkpoint("narrow-before-final")?, narrow);
    sr.type_text("final")?;
    checkpoint!(sr, "narrow-left-elision");
    sr.set_dimensions(12, 12)?;
    checkpoint!(sr, "very-narrow-left-elision");
    sr.set_dimensions(2, 12)?;
    let _ = sr.checkpoint("zero-width-prompt")?;
    sr.set_dimensions(60, 12)?;
    let restored = sr.checkpoint("restored-after-zero-width-prompt")?;
    assert!(
        restored
            .text
            .last()
            .is_some_and(|line| line.contains("> final"))
    );
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}
