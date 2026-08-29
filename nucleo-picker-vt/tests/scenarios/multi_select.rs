use std::{error::Error, num::NonZero};

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent},
};

use super::{ScenarioRunner, lines};

#[test]
fn basic_multi_select() -> Result<(), Box<dyn Error>> {
    let mut sr = ScenarioRunner::start_multi_with_options(
        "basic_multi_select",
        lines(),
        PickerOptions::new().max_selection_count(NonZero::new(3)),
    );
    sr.wait_for_match_complete(24, 24)?;
    for name in ["one", "two", "three"] {
        sr.send(Event::MatchList(MatchListEvent::ToggleUp(1)))?;
        checkpoint!(sr, name);
    }
    sr.send(Event::Select)?;
    assert_eq!(
        sr.finish()?,
        ["item-00 alpha", "item-01 bravo", "item-02 charlie"]
    );
    Ok(())
}
