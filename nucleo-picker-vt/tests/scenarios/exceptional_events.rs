use std::{error::Error, num::NonZero};

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent},
};

use super::{ScenarioRunner, lines};

#[test]
fn exceptional_events_scenario() -> Result<(), Box<dyn Error>> {
    let mut sr = ScenarioRunner::start_multi_with_options(
        "exceptional_events_scenario",
        lines(),
        PickerOptions::new().max_selection_count(NonZero::new(3)),
    );
    sr.wait_for_match_complete(24, 24)?;
    sr.send(Event::MatchList(MatchListEvent::Up(2)))?;
    sr.wait_for(|status| status.selection == Some(2))?;
    sr.send(Event::MatchList(MatchListEvent::QueueAbove(usize::MAX)))?;
    sr.wait_for(|status| status.selection == Some(4) && status.selected_item_count == 3)?;
    sr.send(Event::Select)?;
    assert_eq!(
        sr.finish()?,
        ["item-02 charlie", "item-03 delta", "item-04 echo"]
    );
    Ok(())
}
