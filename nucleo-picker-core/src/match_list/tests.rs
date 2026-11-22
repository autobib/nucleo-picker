use crate::{
    component::{Component, tests::TestSurface},
    event::MatchListEvent,
    render::StrRenderer,
    session::{
        Dimensions, Exit, ModelOptions,
        tests::{Select as Single, frame_options},
    },
};

use super::{MatchList, MatchListView};

type Movement = fn(usize) -> MatchListEvent;

#[test]
fn navigation_survives_empty_dimensions_and_clamps_in_both_orientations() {
    for reversed in [false, true] {
        let mut model = ModelOptions::new()
            .sort_results(false)
            .list_reversed(reversed)
            .model(StrRenderer);
        model.push_batch(["alpha", "beta", "gamma"]);
        model.filter().unwrap();
        let mut session = model.session(Single);
        let (forward, backward): (Movement, Movement) = if reversed {
            (MatchListEvent::Down, MatchListEvent::Up)
        } else {
            (MatchListEvent::Up, MatchListEvent::Down)
        };
        for (event, expected) in [
            (forward(1), 1),
            (forward(usize::MAX), 2),
            (backward(1), 1),
            (backward(usize::MAX), 0),
            (forward(1), 1),
        ] {
            session.events().match_list(event);
            let mut frame = session.prepare_frame(frame_options());
            frame.match_list().resize(Dimensions::default());
            assert_eq!(frame.hovered_rank(), Some(expected));
            frame.complete();
        }
        for height in [1, 3, 0, 3] {
            let mut frame = session.prepare_frame(frame_options());
            frame.match_list().resize(Dimensions { width: 20, height });
            assert_eq!(frame.hovered_rank(), Some(1));
            frame.complete();
        }
        let exit = Exit::Selected(session.events().hovered_id());
        assert_eq!(session.finish(exit), Some(&"beta"));
    }
}

#[test]
fn drawing_resizes_the_list_and_skips_empty_targets() {
    for reversed in [false, true] {
        let mut model = ModelOptions::new()
            .sort_results(false)
            .list_reversed(reversed)
            .model(StrRenderer);
        model.push_batch(["first\nsecond", "third"]);
        model.filter().unwrap();
        let mut list = MatchList::new(&mut model.list_state, &model.chars);
        let mut view = MatchListView {
            list: &mut list,
            engine: &model.engine,
            interface: &Single,
            state: &(),
        };
        view.resize(Dimensions::default());

        for (width, height) in [(20, 1), (20, 3), (0, 3), (20, 0), (20, 4)] {
            let mut target = TestSurface::new(width, height);
            view.draw(&mut target).unwrap();
            assert_eq!(target.touched, width != 0 && height != 0);
            assert_eq!(view.list.hovered_rank(&model.engine), Some(0));
            if width != 0 && height != 0 {
                assert!(
                    target
                        .rows
                        .iter()
                        .any(|row| row.starts_with(model.chars.hover))
                );
            }
        }
    }
}
