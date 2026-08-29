use std::error::Error;

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
};

use super::ScenarioRunner;

fn multiline() -> Vec<String> {
    (0..24)
        .map(|index| match index % 3 {
            0 => format!("item-{index:02}"),
            1 => format!("item-{index:02}\n  detail-{index:02}-a"),
            _ => format!(
                "item-{index:02}\n  detail-{index:02}-a\n  detail-{index:02}-b\n  detail-{index:02}-c"
            ),
        })
        .collect()
}

fn run_multiline_scenario_with_layout(
    scenario_name: &'static str,
    reversed: bool,
) -> Result<(), Box<dyn Error>> {
    let forward_up = !reversed;
    let mut sr = ScenarioRunner::start_with_options(
        scenario_name,
        multiline(),
        PickerOptions::new().reversed(reversed),
    );
    checkpoint!(sr, "default-initial-60x16");

    for rows in [5, 4, 3, 2, 1] {
        sr.set_dimensions(60, rows)?;
        checkpoint!(sr, format!("default-height-60x{rows}"));
    }
    sr.set_dimensions(60, 16)?;
    checkpoint!(sr, "default-restored-height");
    for cols in [5, 4, 3, 2, 1] {
        if cols < 5 {
            sr.set_dimensions(60, 16)?;
            checkpoint!(sr, format!("default-width-reset-{cols}"));
        }
        sr.set_dimensions(cols, 16)?;
        checkpoint!(sr, format!("default-width-{cols}x16"));
    }
    sr.set_dimensions(1, 1)?;
    checkpoint!(sr, "default-size-1x1");
    sr.set_dimensions(60, 16)?;
    checkpoint!(sr, "default-restored-size");

    sr.set_dimensions(12, 40)?;
    checkpoint!(sr, "default-narrow-size");
    sr.send(Event::MatchList(movement(forward_up, 14)))?;
    for item in 14..=18 {
        checkpoint!(sr, format!("default-narrow-12x40-item-{item:02}"));
        if item != 18 {
            sr.send(Event::MatchList(movement(forward_up, 1)))?;
        }
    }
    for item in (14..=17).rev() {
        sr.send(Event::MatchList(movement(!forward_up, 1)))?;
        checkpoint!(sr, format!("default-narrow-12x40-reverse-item-{item:02}"));
    }

    sr.send(Event::MatchList(movement(!forward_up, 14)))?;
    sr.set_dimensions(160, 4)?;
    checkpoint!(sr, "default-wide-size");
    for item in 0..=4 {
        checkpoint!(sr, format!("default-wide-160x4-item-{item:02}"));
        if item != 4 {
            sr.send(Event::MatchList(movement(forward_up, 1)))?;
        }
    }
    for item in (1..=3).rev() {
        sr.send(Event::MatchList(movement(!forward_up, 1)))?;
        checkpoint!(sr, format!("default-wide-160x4-reverse-item-{item:02}"));
    }

    sr.set_dimensions(60, 16)?;
    checkpoint!(sr, "default-restore-60x16");
    sr.send(Event::MatchList(movement(forward_up, 1)))?;
    sr.wait_for(|status| status.selection == Some(2))?;
    sr.send(Event::MatchList(movement(!forward_up, 1)))?;
    checkpoint!(sr, "default-restored-60x16");
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

fn movement(up: bool, count: usize) -> MatchListEvent {
    if up {
        MatchListEvent::Up(count)
    } else {
        MatchListEvent::Down(count)
    }
}

#[test]
fn multiline_default() -> Result<(), Box<dyn Error>> {
    run_multiline_scenario_with_layout("multiline_default", false)
}

#[test]
fn multiline_reversed() -> Result<(), Box<dyn Error>> {
    run_multiline_scenario_with_layout("multiline_reversed", true)
}

#[test]
fn multiline_highlight_viewport() -> Result<(), Box<dyn Error>> {
    let item = [
        "abcdx-head",
        "abcdefghijk界-tail",
        "界界界界界界界界語-tail",
        "short-row",
        "abcdefghijkl界-tail",
        "hidden-spacer",
        "界界界界界界界界終-tail",
        "last-row",
    ]
    .join("\n");
    let options = PickerOptions::new().query("終").reversed(true);
    let mut sr =
        ScenarioRunner::start_with_options("multiline_highlight_viewport", vec![item], options);

    sr.set_dimensions(15, 7)?;
    sr.wait_for_match_complete(1, 1)?;
    checkpoint!(sr, "vertically-clipped-highlight");

    sr.set_dimensions(15, 9)?;
    checkpoint!(sr, "revealed-wide-highlight");

    sr.send(Event::Prompt(PromptEvent::Reset("x語".to_owned())))?;
    sr.wait_for_match_complete(1, 1)?;
    checkpoint!(sr, "distant-highlights-prefer-earlier");

    sr.send(Event::Prompt(PromptEvent::Reset("語".to_owned())))?;
    sr.wait_for_match_complete(1, 1)?;
    checkpoint!(sr, "distant-wide-highlight-only");

    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}
