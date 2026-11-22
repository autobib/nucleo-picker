use std::time::Duration;

use crate::{
    event::{MatchListEvent, PromptEvent},
    interface::{Multi, Single},
    rect::Area,
    render::StrRenderer,
    session::{
        Dimensions, DriverOptions, Geometry, Redraw,
        tests::{TestTerminal, frame_options, update},
    },
};

#[test]
fn counts_fit_the_status_area_and_spinners_advance_on_background_frames() {
    let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
    let injector = driver.injector();
    let mut session = driver.session(Single, Duration::ZERO);
    for (width, background, marker) in [
        (12, false, ' '),
        (12, true, '⠋'),
        (12, false, '⠋'),
        (5, true, '⠙'),
        (4, false, '⠙'),
    ] {
        let mut options = frame_options();
        options.background = background;
        let mut frame = session.prepare_frame(options);
        frame.resize(Geometry {
            dimensions: Dimensions { width, height: 1 },
            status: Area {
                width,
                height: 1,
                ..Area::default()
            },
            ..Geometry::default()
        });
        let mut terminal = TestTerminal::default();
        frame.draw(&mut terminal, Redraw::FULL).unwrap();
        frame.complete();
        let output = String::from_utf8(terminal.output).unwrap();
        assert_eq!(output.contains(&format!("{marker} 0/0")), width >= 5);
        assert_eq!(output.contains("─────"), width == 12);
    }
    drop(injector);
}

#[test]
fn status_redraws_follow_counts_instead_of_list_changes() {
    let mut driver = DriverOptions::new().sort_results(false).driver(StrRenderer);
    driver.push_batch(["alpha", "beta"]);
    driver.filter().unwrap();
    let mut session = driver.session(Multi::new(), Duration::ZERO);
    let mut initial = session.prepare_frame(frame_options());
    initial.resize(Geometry::default());
    initial.complete();

    use MatchListEvent::*;
    for (events, selected, changed) in [
        ([Some(Up(1)), None, None], 0, false),
        ([Some(ToggleDown(0)), Some(ToggleDown(0)), None], 0, false),
        ([Some(ToggleDown(0)), None, None], 1, true),
        (
            [Some(UnqueueAll), Some(Down(1)), Some(ToggleDown(0))],
            1,
            false,
        ),
    ] {
        {
            let mut batch = session.events();
            for event in events.into_iter().flatten() {
                batch.match_list(event);
            }
        }
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.redraw().status, changed);
        assert_eq!(frame.view().queue_status().unwrap().count, selected);
        frame.complete();
    }

    session
        .events()
        .prompt(PromptEvent::Reset("^alpha$".to_owned()));
    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().status);
    assert_eq!(frame.view().context().matches().len(), 1);
    frame.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.redraw().status);
    idle.complete();
}
