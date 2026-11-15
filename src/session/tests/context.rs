use std::time::Duration;

use super::{Select, frame_options};
use crate::{
    DriverOptions,
    event::{MatchListEvent, PromptEvent},
    render::StrRenderer,
    session::{Context, Exit, Interface, ItemId, QueueEvent},
};

struct Observation {
    rank: Option<u32>,
    hovered: Option<ItemId>,
    matches: Vec<ItemId>,
}

impl Observation {
    fn new(cx: &Context<'_, String>) -> Self {
        Self {
            rank: cx.rank(),
            hovered: cx.hovered().map(|item| item.id()),
            matches: cx.matches().ids().collect(),
        }
    }
}

struct Probe<'a>(&'a mut Vec<Observation>);

impl Interface<String> for Probe<'_> {
    type State = ();
    type Output<'items> = Option<&'items String>;

    fn init(&mut self, cx: &Context<'_, String>) {
        self.0.push(Observation::new(cx));
    }

    fn on_queue(
        &mut self,
        _: &mut (),
        _: QueueEvent<'_, String>,
        cx: &Context<'_, String>,
    ) -> bool {
        self.0.push(Observation::new(cx));
        false
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        context: Context<'items, String>,
    ) -> Self::Output<'items> {
        self.0.push(Observation::new(&context));
        Select.finish((), exit, context)
    }
}

#[test]
fn event_contexts_and_borrowed_output_use_navigation_without_publishing_matches() {
    let mut driver = DriverOptions::new()
        .sort_results(false)
        .query("ta")
        .driver(StrRenderer);
    driver.push_batch(["alpha", "beta", "delta"].map(str::to_owned));
    driver.filter().unwrap();
    let expected_ids: Vec<_> = driver.matches().ids().collect();
    let selected_ptr = driver.matches().get(1).unwrap().value() as *const String;

    let selected = {
        let mut observations = Vec::new();
        let mut interface = Probe(&mut observations);
        let mut session = driver.session(&mut interface, Duration::ZERO);
        {
            let mut batch = session.events();
            batch.match_list(MatchListEvent::UnqueueAll);
            batch.prompt(PromptEvent::Reset("absent".to_owned()));
            assert!(!batch.prompt_is_empty());
        }
        let exit = {
            let mut batch = session.events();
            batch.match_list(MatchListEvent::Up(1));
            batch.prompt(PromptEvent::Left(1));
            batch.match_list(MatchListEvent::UnqueueAll);
            batch.selection_exit().unwrap()
        };
        assert_eq!(exit, Exit::Selected(Some(expected_ids[1])));
        let frame = session.prepare_frame(frame_options());
        assert_eq!(frame.view().query(), "absent");
        frame.complete();
        let selected = session.finish(exit).unwrap();

        assert_eq!(observations.len(), 4);
        for (observation, rank) in observations.iter().zip([0, 0, 1, 1]) {
            assert_eq!(observation.rank, Some(rank));
            assert_eq!(observation.hovered, Some(expected_ids[rank as usize]));
            assert_eq!(observation.matches, expected_ids);
        }
        selected
    };

    assert_eq!(selected, "delta");
    assert_eq!(selected as *const String, selected_ptr);
    assert_eq!(driver.query(), "absent");
    assert!(driver.filter().unwrap().is_empty());
}
