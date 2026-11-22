use std::{num::NonZero, time::Duration};

use crate::{
    event::MatchListEvent,
    interface::{Multi, Single},
    rect::Area,
    render::StrRenderer,
    session::{DriverOptions, Geometry, tests::frame_options},
};

type Movement = fn(usize) -> MatchListEvent;

#[test]
fn navigation_survives_empty_geometry_and_clamps_in_both_orientations() {
    for reversed in [false, true] {
        let mut driver = DriverOptions::new()
            .sort_results(false)
            .reversed(reversed)
            .driver(StrRenderer);
        driver.push_batch(["alpha", "beta", "gamma"]);
        driver.filter().unwrap();
        let mut session = driver.session(Single, Duration::ZERO);
        let (forward, backward): (Movement, Movement) = if reversed {
            (MatchListEvent::Down, MatchListEvent::Up)
        } else {
            (MatchListEvent::Up, MatchListEvent::Down)
        };
        for (event, expected) in [
            (forward(1), "beta"),
            (forward(usize::MAX), "gamma"),
            (backward(1), "beta"),
            (backward(usize::MAX), "alpha"),
            (forward(1), "beta"),
        ] {
            session.events().match_list(event);
            let mut frame = session.prepare_frame(frame_options());
            frame.resize(Geometry::default());
            assert_eq!(frame.view().context().hovered().unwrap().value(), &expected);
            frame.complete();
        }
        for height in [1, 3, 0, 3] {
            let mut frame = session.prepare_frame(frame_options());
            frame.resize(Geometry {
                list: Area {
                    width: 20,
                    height,
                    ..Area::default()
                },
                ..Geometry::default()
            });
            assert_eq!(frame.view().context().hovered().unwrap().value(), &"beta");
            frame.complete();
        }
        let exit = session.events().selection_exit().unwrap();
        assert_eq!(session.finish(exit), Some(&"beta"));
    }
}

#[test]
fn a_full_queue_blocks_insertion_movement_but_allows_removal_movement() {
    for reversed in [false, true] {
        let mut driver = DriverOptions::new()
            .sort_results(false)
            .reversed(reversed)
            .driver(StrRenderer);
        driver.push_batch(["alpha", "beta", "gamma"]);
        driver.filter().unwrap();
        let mut session = driver.session(
            Multi::new().max_selection_count(NonZero::new(1)),
            Duration::ZERO,
        );
        let toggle = if reversed {
            MatchListEvent::ToggleDown
        } else {
            MatchListEvent::ToggleUp
        };
        for (reset, count, hovered, queued) in [
            (false, 1, "beta", 1),
            (false, 1, "beta", 1),
            (true, 2, "gamma", 0),
        ] {
            {
                let mut batch = session.events();
                if reset {
                    batch.match_list(MatchListEvent::Reset);
                }
                batch.match_list(toggle(count));
            }
            let frame = session.prepare_frame(frame_options());
            assert_eq!(frame.view().context().hovered().unwrap().value(), &hovered);
            assert_eq!(frame.view().queue_status().unwrap().count, queued);
            frame.complete();
        }
        let exit = session.events().selection_exit().unwrap();
        assert!(session.finish(exit).iter().copied().eq(["gamma"]));
    }
}

#[test]
#[expect(deprecated)]
fn range_queueing_traverses_existing_entries_at_capacity() {
    for reversed in [false, true] {
        let mut driver = DriverOptions::new()
            .sort_results(false)
            .reversed(reversed)
            .driver(StrRenderer);
        driver.push_batch(["alpha", "beta", "gamma"]);
        driver.filter().unwrap();
        let mut session = driver.session(
            Multi::new().max_selection_count(NonZero::new(2)),
            Duration::ZERO,
        );
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
            batch.selection_exit().unwrap()
        };
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.view().context().hovered().unwrap().value(), &"beta");
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
