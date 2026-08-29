use std::{error::Error, num::NonZero};

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
};

use super::{ScenarioRunner, lines};

#[test]
fn basic_options() -> Result<(), Box<dyn Error>> {
    let options = PickerOptions::new()
        .query("item-1")
        .reversed(true)
        .sort_results(false)
        .reverse_items(true)
        .highlight_padding(8)
        .scroll_padding(2)
        .prompt_padding(0)
        .max_selection_count(NonZero::new(2));
    let mut sr = ScenarioRunner::start_multi_with_options("basic_options", lines(), options);

    sr.wait_for_match_complete(12, 24)?;
    checkpoint!(sr, "custom-options");

    sr.send(Event::Prompt(PromptEvent::Reset("ia".to_owned())))?;
    sr.wait_for(|status| status.query == "ia" && !status.matching && !status.injecting)?;
    checkpoint!(sr, "sort-results-disabled");

    sr.set_dimensions(12, 8)?;
    sr.send(Event::Prompt(PromptEvent::Reset("victor".to_owned())))?;
    sr.wait_for_match_complete(1, 24)?;
    checkpoint!(sr, "highlight-padding");

    sr.set_dimensions(24, 8)?;
    sr.send(Event::Prompt(PromptEvent::Reset(String::new())))?;
    sr.wait_for_match_complete(24, 24)?;
    sr.send(Event::MatchList(MatchListEvent::Down(2)))?;
    checkpoint!(sr, "scroll-padding-before-scroll");
    sr.send(Event::MatchList(MatchListEvent::Down(1)))?;
    checkpoint!(sr, "scroll-padding-after-scroll");

    sr.set_dimensions(60, 16)?;
    sr.send(Event::Prompt(PromptEvent::Reset("item-1".to_owned())))?;
    sr.wait_for_match_complete(12, 24)?;
    sr.send(Event::MatchList(MatchListEvent::Reset))?;
    sr.send(Event::MatchList(MatchListEvent::ToggleDown(1)))?;
    sr.send(Event::MatchList(MatchListEvent::ToggleDown(1)))?;
    sr.send(Event::Select)?;
    assert_eq!(sr.finish()?, ["item-19 tango", "item-21 victor"]);
    Ok(())
}
