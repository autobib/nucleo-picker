use std::error::Error;

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
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
