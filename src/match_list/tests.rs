use std::num::NonZero;

use crate::{
    component::{Component, tests::TestSurface},
    event::MatchListEvent,
    interface::{Multi, Single},
    render::StrRenderer,
    session::{Dimensions, Exit, ModelOptions, tests::frame_options},
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

#[test]
fn a_full_queue_blocks_insertion_movement_but_allows_removal_movement() {
    for reversed in [false, true] {
        let mut model = ModelOptions::new()
            .sort_results(false)
            .list_reversed(reversed)
            .model(StrRenderer);
        model.push_batch(["alpha", "beta", "gamma"]);
        model.filter().unwrap();
        let mut session = model.session(Multi::new().max_selection_count(NonZero::new(1)));
        let toggle = if reversed {
            MatchListEvent::ToggleDown
        } else {
            MatchListEvent::ToggleUp
        };
        for (reset, count, rank, queued) in [(false, 1, 1, 1), (false, 1, 1, 1), (true, 2, 2, 0)] {
            {
                let mut batch = session.events();
                if reset {
                    batch.match_list(MatchListEvent::Reset);
                }
                batch.match_list(toggle(count));
            }
            let frame = session.prepare_frame(frame_options());
            assert_eq!(frame.hovered_rank(), Some(rank));
            assert_eq!(frame.queue_status().unwrap().count, queued);
            frame.complete();
        }
        let exit = Exit::Selected(session.events().hovered_id());
        assert!(session.finish(exit).iter().copied().eq(["gamma"]));
    }
}

#[test]
#[expect(deprecated)]
fn range_queueing_traverses_existing_entries_at_capacity() {
    for reversed in [false, true] {
        let mut model = ModelOptions::new()
            .sort_results(false)
            .list_reversed(reversed)
            .model(StrRenderer);
        model.push_batch(["alpha", "beta", "gamma"]);
        model.filter().unwrap();
        let mut session = model.session(Multi::new().max_selection_count(NonZero::new(2)));
        let (toggle, queue): (Movement, Movement) = if reversed {
            (MatchListEvent::ToggleDown, MatchListEvent::QueueBelow)
        } else {
            (MatchListEvent::ToggleUp, MatchListEvent::QueueAbove)
        };
        let exit = {
            let mut batch = session.events();
            batch.match_list(toggle(1));
            batch.match_list(toggle(0));
            batch.match_list(MatchListEvent::Reset);
            batch.match_list(queue(usize::MAX));
            Exit::Selected(batch.hovered_id())
        };
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.hovered_rank(), Some(1));
        frame.complete();
        assert!(
            session
                .finish(exit)
                .iter_selected_order()
                .copied()
                .eq(["alpha", "beta"])
        );
    }
}
