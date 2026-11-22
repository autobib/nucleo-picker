use std::time::Instant;

use super::{Select, frame_options, model};
use crate::{
    event::{MatchListEvent, PromptEvent},
    interface::Interface,
    preview::PreviewEvent,
    render::StrRenderer,
    session::{Components, Context, Exit, ItemId, ModelOptions, QueueEvent},
};

struct Observation {
    rank: Option<u32>,
    hovered: Option<ItemId>,
    matches: Vec<ItemId>,
    active_components: Components,
}

impl Observation {
    fn new(cx: &Context<'_, String>) -> Self {
        Self {
            rank: cx.hovered_rank(),
            hovered: cx.hovered().map(|item| item.id()),
            matches: cx.matches().ids().collect(),
            active_components: cx.active_components(),
        }
    }
}

struct Probe<'a, const PREVIEW: bool>(&'a mut Vec<Observation>);

impl<const PREVIEW: bool> Interface<String> for Probe<'_, PREVIEW> {
    type State = ();
    type Output<'items> = Option<&'items String>;

    const PREVIEW: bool = PREVIEW;

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

    fn on_restart(&mut self, _: &mut (), cx: &Context<'_, String>) {
        self.0.push(Observation::new(cx));
    }

    fn preview_event(
        &mut self,
        _: &mut (),
        _: PreviewEvent,
        cx: &Context<'_, String>,
    ) -> Components {
        self.0.push(Observation::new(cx));
        Components::NONE
    }

    fn before_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, String>,
        _: bool,
        _: Instant,
    ) -> Components {
        self.0.push(Observation::new(cx));
        Components::NONE
    }

    fn after_frame(&mut self, _: &mut (), cx: &Context<'_, String>, _: Instant) -> Components {
        self.0.push(Observation::new(cx));
        Components::NONE
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
    let mut model = ModelOptions::new()
        .sort_results(false)
        .query("ta")
        .model(StrRenderer);
    model.push_batch(["alpha", "beta", "delta"].map(str::to_owned));
    model.filter().unwrap();
    let expected_ids: Vec<_> = model.matches().ids().collect();
    let selected_ptr = model.matches().get(1).unwrap().value() as *const String;

    let selected = {
        let mut observations = Vec::new();
        let mut interface = Probe::<false>(&mut observations);
        let mut session = model.session(&mut interface);
        {
            let mut batch = session.events();
            batch.match_list(MatchListEvent::UnqueueAll);
            batch.prompt(PromptEvent::Reset("absent".to_owned()));
            assert!(!batch.query().is_empty());
        }
        let hovered = {
            let mut batch = session.events();
            batch.match_list(MatchListEvent::Up(1));
            batch.match_list(MatchListEvent::UnqueueAll);
            batch.hovered_id()
        };
        assert_eq!(hovered, Some(expected_ids[1]));
        let selected = session.finish(Exit::Selected(hovered)).unwrap();

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
    assert_eq!(model.query(), "absent");
    assert!(model.filter().unwrap().is_empty());
}

#[test]
fn hooks_observe_all_component_visibility_flags_and_normalize_unsupported_preview() {
    fn check<const PREVIEW: bool>() {
        let initial = Components {
            preview: PREVIEW,
            ..Components::ALL
        };
        for mask in 0..16 {
            let components = Components {
                prompt: mask & 1 != 0,
                match_list: mask & 2 != 0,
                status: mask & 4 != 0,
                preview: mask & 8 != 0,
            };
            let visible = Components {
                preview: components.preview && PREVIEW,
                ..components
            };
            let mut model = model(["alpha"]);
            let mut observations = Vec::new();
            let mut session = model.session(Probe::<PREVIEW>(&mut observations));
            {
                let mut batch = session.events();
                batch.match_list(MatchListEvent::UnqueueAll);
                batch.set_active_components(components);
                batch.match_list(MatchListEvent::UnqueueAll);
                batch.preview(PreviewEvent::Refresh);
                drop(batch.restart());
            }
            session.prepare_frame(frame_options()).complete();
            assert!(session.finish(Exit::Quit).is_none());

            assert_eq!(observations.len(), 7 + usize::from(PREVIEW));
            for (index, observation) in observations.iter().enumerate() {
                assert_eq!(
                    observation.active_components,
                    if index < 2 { initial } else { visible },
                    "preview={PREVIEW}, mask={mask}, observation={index}",
                );
            }
        }
    }

    check::<false>();
    check::<true>();
}
