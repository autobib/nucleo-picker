use std::error::Error;

use nucleo_picker::{
    PickerOptions,
    event::{Event, LayoutEvent, MatchListEvent, PromptEvent},
};

use super::{ScenarioRunner, lines};

#[test]
fn basic() -> Result<(), Box<dyn Error>> {
    let mut sr = ScenarioRunner::start_with_options("basic", lines(), PickerOptions::new());
    checkpoint!(sr, "initial");

    sr.type_text("item-1")?;
    checkpoint!(sr, "filtered");
    sr.send(Event::Prompt(PromptEvent::Left(1)))?;
    sr.send(Event::Prompt(PromptEvent::Backspace(1)))?;
    checkpoint!(sr, "edited");
    sr.type_text("zzz")?;
    checkpoint!(sr, "no-match");
    sr.send(Event::Prompt(PromptEvent::ToStart))?;
    sr.send(Event::Prompt(PromptEvent::ClearAfter))?;
    sr.wait_for_match_complete(24, 24)?;
    sr.send(Event::MatchList(MatchListEvent::Up(3)))?;
    checkpoint!(sr, "selection-03");
    sr.send(Event::MatchList(MatchListEvent::Down(1)))?;
    checkpoint!(sr, "selection-02");
    sr.send(Event::Prompt(PromptEvent::Reset("item-23".to_owned())))?;
    sr.wait_for_match_complete(1, 24)?;
    sr.send(Event::Select)?;
    assert_eq!(sr.finish()?, ["item-23 xray"]);
    Ok(())
}

#[test]
fn toggle_status() -> Result<(), Box<dyn Error>> {
    for reversed in [false, true] {
        let mut sr = ScenarioRunner::start_with_options(
            if reversed {
                "toggle_status_reversed"
            } else {
                "toggle_status"
            },
            lines(),
            PickerOptions::new().reversed(reversed),
        );
        sr.set_dimensions(30, 5)?;
        sr.wait_for_match_complete(24, 24)?;
        let shown = sr.checkpoint("shown")?;
        assert_eq!(
            shown
                .text
                .iter()
                .filter(|line| line.contains("item-"))
                .count(),
            3
        );
        checkpoint!(sr, "shown");

        sr.send(Event::Layout(LayoutEvent::ToggleStatus))?;
        let hidden = sr.checkpoint("hidden")?;
        assert!(hidden.text.iter().all(|line| !line.contains("24/24")));
        assert_eq!(
            hidden
                .text
                .iter()
                .filter(|line| line.contains("item-"))
                .count(),
            4
        );
        assert_eq!(hidden.cursor, shown.cursor);
        checkpoint!(sr, "hidden");

        sr.send(Event::Layout(LayoutEvent::ToggleStatus))?;
        assert_eq!(sr.checkpoint("restored")?, shown);
        checkpoint!(sr, "restored");

        sr.send(Event::Layout(LayoutEvent::ToggleStatus))?;
        sr.send(Event::Prompt(PromptEvent::Reset("item-23".to_owned())))?;
        sr.wait_for_match_complete(1, 24)?;
        sr.set_dimensions(30, 2)?;
        let small = sr.checkpoint("hidden after filtering and resizing")?;
        assert!(small.text.iter().any(|line| line.contains("item-23 xray")));
        assert!(small.text.iter().all(|line| !line.contains("1/24")));

        sr.set_dimensions(30, 5)?;
        sr.checkpoint("grown while hidden")?;
        sr.send(Event::Layout(LayoutEvent::ToggleStatus))?;
        let current = sr.checkpoint("current counts")?;
        assert!(current.text[if reversed { 1 } else { 3 }].contains("1/24"));
        sr.send(Event::Select)?;
        assert_eq!(sr.finish()?, ["item-23 xray"]);
    }
    Ok(())
}

#[test]
fn prompt_scroll_at_column_zero() -> Result<(), Box<dyn Error>> {
    let mut sr = ScenarioRunner::start_with_options(
        "prompt_scroll_at_column_zero",
        vec!["abc"],
        PickerOptions::new().query("abc"),
    );
    sr.set_dimensions(3, 3)?;
    sr.wait_for_match_complete(1, 1)?;
    let before = sr.checkpoint("before-left")?;
    assert_eq!(before.text.last().unwrap(), "> c");

    sr.send(Event::Prompt(PromptEvent::Left(1)))?;
    let after = sr.checkpoint("after-left")?;
    assert_eq!(after.text, before.text);
    assert!(after.row_flags.is_empty());
    assert_eq!(after.cursor.position, before.cursor.position);
    checkpoint!(sr, "after-left");

    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}
