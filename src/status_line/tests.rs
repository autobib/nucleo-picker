use crate::{
    component::tests::TestSurface,
    event::{MatchListEvent, PromptEvent},
    interface::{Multi, Single},
    render::StrRenderer,
    session::{
        DriverOptions,
        tests::{frame_options, update},
    },
};

#[test]
fn counts_fit_the_status_area_and_spinners_advance_on_background_frames() {
    let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
    let injector = driver.injector();
    let mut session = driver.session(Single);
    for (width, height, background, marker) in [
        (12, 1, false, ' '),
        (0, 1, false, ' '),
        (12, 0, false, ' '),
        (12, 1, true, '⠋'),
        (12, 1, false, '⠋'),
        (5, 1, true, '⠙'),
        (4, 1, false, '⠙'),
    ] {
        let mut frame = session.prepare_frame(frame_options().background(background));
        let mut target = TestSurface::new(width, height);
        frame.status().draw(&mut target).unwrap();
        frame.complete();
        assert_eq!(target.touched, width != 0 && height != 0);
        let output = target.rows.pop().unwrap_or_default();
        assert_eq!(
            output.contains(&format!("{marker} 0/0")),
            width >= 5 && height != 0
        );
        assert_eq!(output.contains("─────"), width == 12 && height != 0);
    }
    drop(injector);
}

#[test]
fn status_redraws_follow_counts_instead_of_list_changes() {
    let mut driver = DriverOptions::new().sort_results(false).driver(StrRenderer);
    driver.push_batch(["alpha", "beta"]);
    driver.filter().unwrap();
    let mut session = driver.session(Multi::new());
    let initial = session.prepare_frame(frame_options());
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
        assert_eq!(frame.queue_status().unwrap().count, selected);
        frame.complete();
    }

    session
        .events()
        .prompt(PromptEvent::Reset("^alpha$".to_owned()));
    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().status);
    assert_eq!(frame.match_status().matched_item_count, 1);
    frame.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.redraw().status);
    idle.complete();
}
