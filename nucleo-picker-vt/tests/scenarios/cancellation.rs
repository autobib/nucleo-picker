use std::error::Error;

use nucleo_picker::{
    PickerOptions,
    event::{Event, PromptEvent},
};

use super::{ScenarioRunner, lines};

#[test]
fn basic_cancellation() -> Result<(), Box<dyn Error>> {
    let mut sr =
        ScenarioRunner::start_with_options("basic_cancellation", lines(), PickerOptions::new());
    sr.type_text("item-2")?;
    checkpoint!(sr, "filtered");
    sr.send(Event::Prompt(PromptEvent::ClearBefore))?;
    sr.send(Event::QuitPromptEmpty)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}
