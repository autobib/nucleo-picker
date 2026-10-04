use std::{convert::Infallible, error::Error, time::Duration};

use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
    preview::{Preview, PreviewRequest, PreviewResponse},
};
use nucleo_picker_vt::{Driver, PaneSnapshot};
use unicode_width::UnicodeWidthStr;

use super::{ScenarioRunner, WAIT, lines};

struct ItemPreview;

impl Preview<String> for ItemPreview {
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        item: &String,
        request: PreviewRequest,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Infallible> {
        let mut buffer = request.ready();
        buffer.push_str(item);
        Ok(PreviewResponse::Ready(buffer))
    }
}

fn start(options: PickerOptions) -> ScenarioRunner {
    crossterm::style::force_color_output(true);
    ScenarioRunner {
        scenario_name: "basic",
        driver: Driver::start_with_preview(lines(), options, ItemPreview),
        timeout: WAIT,
        checkpoint_sequence: 0,
    }
}

fn assert_pane(snapshot: &PaneSnapshot, reversed: bool) {
    let width = usize::from(snapshot.size.cols / 2);
    let column = usize::from(snapshot.size.cols) - width;
    let height = usize::from(snapshot.size.rows);
    for (row, line) in snapshot.text.iter().enumerate() {
        let expected = if row == 0 {
            format!("╭{}╮", "─".repeat(width - 2))
        } else if row == height - 1 {
            format!("╰{}╯", "─".repeat(width - 2))
        } else {
            format!("│{}│", " ".repeat(width - 2))
        };
        let prefix = line
            .strip_suffix(&expected)
            .unwrap_or_else(|| panic!("unexpected row: {line:?}"));
        assert_eq!(prefix.width(), column);
    }
    assert_eq!(snapshot.text.len(), height);
    assert!(
        snapshot
            .styles
            .iter()
            .all(|span| usize::from(span.end) <= column)
    );
    assert!(
        snapshot
            .row_flags
            .iter()
            .all(|row| !row.wrapped && !row.continuation)
    );
    let cursor = snapshot.cursor.position.unwrap();
    assert!(usize::from(cursor.x) < column);
    assert_eq!(cursor.y, if reversed { 0 } else { snapshot.size.rows - 1 });
    assert!(!snapshot.cursor.pending_wrap);
}

#[test]
fn basic() -> Result<(), Box<dyn Error>> {
    let mut sr = start(PickerOptions::new());
    sr.wait_for_match_complete(24, 24)?;
    assert_pane(&sr.checkpoint("blank")?, false);
    // TODO: This snapshot will change when preview contents are rendered.
    checkpoint!(sr, "blank");
    sr.send(Event::Select)?;
    assert_eq!(sr.finish()?, ["item-00 alpha"]);
    Ok(())
}

#[test]
fn pane_survives_updates_and_resizes() -> Result<(), Box<dyn Error>> {
    for reversed in [false, true] {
        let mut sr = start(PickerOptions::new().reversed(reversed).highlight_line(true));
        sr.set_dimensions(23, 7)?;
        sr.wait_for_match_complete(24, 24)?;
        assert_pane(&sr.checkpoint("initial")?, reversed);

        sr.send(Event::Prompt(PromptEvent::Reset(
            "a long query containing 東京".to_owned(),
        )))?;
        sr.wait_for_match_complete(0, 24)?;
        assert_pane(&sr.checkpoint("no matches")?, reversed);
        sr.send(Event::Prompt(PromptEvent::Left(1)))?;
        assert_pane(&sr.checkpoint("prompt only")?, reversed);
        sr.send(Event::Redraw)?;
        assert_pane(&sr.checkpoint("full redraw")?, reversed);

        sr.send(Event::Prompt(PromptEvent::Reset(String::new())))?;
        sr.wait_for_match_complete(24, 24)?;
        sr.send(Event::MatchList(if reversed {
            MatchListEvent::Down(1)
        } else {
            MatchListEvent::Up(1)
        }))?;
        assert_pane(&sr.checkpoint("selection")?, reversed);

        for (width, height) in [(5, 7), (23, 2), (1, 1)] {
            sr.set_dimensions(width, height)?;
            let snapshot = sr.checkpoint("hidden")?;
            assert!(
                snapshot
                    .text
                    .iter()
                    .all(|row| !row.contains('╭') && !row.contains('│'))
            );
            assert!(snapshot.cursor.position.unwrap().x < width);
        }
        for (width, height) in [(6, 3), (61, 16)] {
            sr.set_dimensions(width, height)?;
            assert_pane(&sr.checkpoint("restored")?, reversed);
        }

        sr.send(Event::Quit)?;
        assert!(sr.finish()?.is_empty());
    }
    Ok(())
}
