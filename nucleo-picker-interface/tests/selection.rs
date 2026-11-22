use std::num::NonZero;

use nucleo_picker_core::{
    event::MatchListEvent,
    render::StrRenderer,
    session::{Exit, Model, ModelOptions},
};
use nucleo_picker_interface::Multi;

fn model(reversed: bool) -> Model<&'static str, StrRenderer> {
    let mut model = ModelOptions::new()
        .sort_results(false)
        .list_reversed(reversed)
        .model(StrRenderer);
    model.push_batch(["alpha", "beta", "gamma"]);
    model.filter().unwrap();
    model
}

#[test]
fn a_full_queue_blocks_insertion_movement_but_allows_removal_movement() {
    for reversed in [false, true] {
        let mut model = model(reversed);
        let mut session = model.session(Multi::new().max_selection_count(NonZero::new(1)));
        let toggle = if reversed {
            MatchListEvent::ToggleDown
        } else {
            MatchListEvent::ToggleUp
        };
        for (reset, count, rank, queued) in [(false, 1, 1, 1), (false, 1, 1, 1), (true, 2, 2, 0)] {
            let mut batch = session.events();
            if reset {
                batch.match_list(MatchListEvent::Reset);
            }
            batch.match_list(toggle(count));
            assert_eq!(batch.queue_status().unwrap().count, queued);
            drop(batch);
            assert_eq!(session.hovered_rank(), Some(rank));
        }
        let exit = Exit::Selected(session.events().hovered_id());
        assert!(session.finish(exit).into_iter().copied().eq(["gamma"]));
    }
}

#[test]
#[expect(deprecated)]
fn range_queueing_traverses_existing_entries_at_capacity() {
    type Movement = fn(usize) -> MatchListEvent;

    for reversed in [false, true] {
        let mut model = model(reversed);
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
        assert_eq!(session.hovered_rank(), Some(1));
        assert!(
            session
                .finish(exit)
                .into_iter()
                .copied()
                .eq(["alpha", "beta"])
        );
    }
}
