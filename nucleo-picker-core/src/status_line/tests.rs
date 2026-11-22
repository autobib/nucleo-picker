use crate::{
    component::tests::TestSurface,
    event::{MatchListEvent, PromptEvent},
    render::StrRenderer,
    session::{
        ModelOptions,
        tests::{Queued, Select as Single, frame_options, update},
    },
};

#[test]
fn counts_fit_the_status_area_and_spinners_advance_on_background_frames() {
    let mut model = ModelOptions::new().model::<&str, _>(StrRenderer);
    let injector = model.injector();
    let mut session = model.session(Single);
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
    let mut model = ModelOptions::new().sort_results(false).model(StrRenderer);
    model.push_batch(["alpha", "beta"]);
    model.filter().unwrap();
    let [alpha, beta] = [0, 1].map(|rank| model.matches().get(rank).unwrap().id());
    let queued = Cell::new(None);
    let mut session = model.session(Queued(&queued));
    let initial = session.prepare_frame(frame_options());
    initial.complete();

    for (event, selection, changed) in [
        (MatchListEvent::Up(1), None, false),
        (MatchListEvent::Up(0), Some(beta), true),
        (MatchListEvent::Down(1), Some(alpha), false),
        (MatchListEvent::Up(0), None, true),
    ] {
        session.events().match_list(event);
        queued.set(selection);
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.redraw().status, changed);
        assert_eq!(
            frame.queue_status().unwrap().count,
            u32::from(queued.get().is_some())
        );
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
use std::cell::Cell;
