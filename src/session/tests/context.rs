use std::time::{Duration, Instant};

use super::{TestEvents, TestTerminal};
use crate::{
    PickerOptions,
    component::NoPreview,
    event::{Event, MatchListEvent, PromptEvent},
    match_list::{QueueEvent, Queued, SelectionTarget},
    render::StrRenderer,
    session::{Context, ItemId},
};

#[test]
fn context_tracks_rank_identity_and_match_publication() {
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .query("ta")
        .picker(StrRenderer);
    let cx = Context::new(&picker.core.engine);
    assert_eq!(cx.query(), "ta");
    #[cfg(feature = "preview")]
    assert!(cx.preview_size().is_none());
    let revision = cx.match_revision();
    picker.push_batch(["alpha", "beta", "delta"]);
    assert!(Context::new(&picker.core.engine).match_revision() == revision);
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine);
    assert!(cx.hovered().is_none());
    assert_eq!(cx.rank(), None);
    let cx = cx.with_rank(Some(1));
    let hovered = cx.hovered().unwrap();
    assert_eq!(cx.rank(), Some(1));
    assert_eq!(cx.match_ids().nth(1), Some(hovered.id()));
    assert_eq!(hovered.value(), &"delta");
    assert_eq!(hovered.id(), picker.core.engine.item_id(2));
    let published = cx.match_revision();
    assert!(published != revision);
    picker.filter().unwrap();
    assert!(Context::new(&picker.core.engine).match_revision() == published);

    picker.update_query("delta");
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert_eq!(cx.query(), "delta");
    assert_eq!(cx.hovered().unwrap().value(), &"beta");
    assert_eq!(cx.matches().len(), 2);
    assert!(cx.match_revision() == published);

    let start = Instant::now();
    loop {
        let before = Context::new(&picker.core.engine).match_revision();
        let status = picker.update_matches(Duration::from_millis(5));
        let after = Context::new(&picker.core.engine).match_revision();
        assert_eq!(before != after, status.items_changed);
        if !status.matching {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert!(cx.match_revision() != published);
    assert_eq!(cx.hovered().unwrap().value(), &"delta");
    let before_restart = cx.match_revision();
    let old_id = cx.hovered().unwrap().id();

    picker.restart();
    let cx = Context::new(&picker.core.engine);
    assert!(cx.match_revision() != before_restart);
    assert_eq!(cx.matches().len(), 0);
    assert!(cx.item(old_id).is_none());
    let restarted = cx.match_revision();
    picker.restart();
    assert!(Context::new(&picker.core.engine).match_revision() != restarted);
    picker.push_batch(["replacement"]);
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert!(cx.match_revision() != restarted);
    assert_eq!(cx.hovered().unwrap().value(), &"replacement");

    picker.update_query("absent");
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert!(cx.hovered().is_none());
    assert_eq!(cx.rank(), None);
}

struct Observation {
    hovered: Option<ItemId>,
    rank: Option<u32>,
    query: String,
    revision: u64,
    matches: Vec<ItemId>,
    #[cfg(feature = "preview")]
    preview_size: Option<(u16, u16)>,
}

impl Observation {
    fn new<T: Send + Sync + 'static>(cx: &Context<'_, T>) -> Self {
        Self {
            hovered: cx.hovered().map(|item| item.id()),
            rank: cx.rank(),
            query: cx.query().to_owned(),
            revision: cx.match_revision(),
            matches: cx.match_ids().collect(),
            #[cfg(feature = "preview")]
            preview_size: cx.preview_size().map(|size| (size.width, size.height)),
        }
    }
}

struct QueueProbe<'a>(&'a mut Vec<Observation>);

impl Queued for QueueProbe<'_> {
    type Output<'items, T: Send + Sync + 'static> = Option<&'items T>;

    fn on_queue<T: Send + Sync + 'static>(
        &mut self,
        _: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        self.0.push(Observation::new(cx));
        false
    }

    fn on_select<'items, T: Send + Sync + 'static>(
        self,
        target: SelectionTarget,
        cx: &Context<'items, T>,
    ) -> Self::Output<'items, T> {
        self.0.push(Observation::new(cx));
        match target {
            SelectionTarget::Hovered(id) => Some(cx.item(id).unwrap().value()),
            _ => None,
        }
    }
}

#[test]
fn events_observe_buffered_navigation_and_only_submitted_queries() {
    for reversed in [false, true] {
        let mut picker = PickerOptions::new()
            .sort_results(false)
            .query("ta")
            .reversed(reversed)
            .frame_interval(Duration::from_secs(5))
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta", "delta"]);
        picker.filter().unwrap();
        let ids: Vec<_> = Context::new(&picker.core.engine).match_ids().collect();
        let navigate = if reversed {
            MatchListEvent::Down(1)
        } else {
            MatchListEvent::Up(1)
        };
        let mut events = [
            Event::MatchList(MatchListEvent::UnqueueAll),
            Event::Prompt(PromptEvent::Reset("absent".to_owned())),
            Event::Prompt(PromptEvent::Left(1)),
            Event::MatchList(navigate),
            Event::MatchList(MatchListEvent::UnqueueAll),
            Event::Select,
        ]
        .into_iter();
        let mut observations = Vec::new();
        let selected = picker
            .core
            .pick_impl(
                TestEvents(|_| Ok(events.next().unwrap())),
                &mut TestTerminal::default(),
                QueueProbe(&mut observations),
                NoPreview,
            )
            .unwrap();
        assert_eq!(selected, Some(&"delta"));
        assert_eq!(observations.len(), 3);
        for (observation, rank, query) in [
            (&observations[0], 0, "ta"),
            (&observations[1], 1, "ta"),
            (&observations[2], 1, "absent"),
        ] {
            assert_eq!(observation.rank, Some(rank));
            assert_eq!(observation.hovered, Some(ids[rank as usize]));
            assert_eq!(observation.query, query);
            assert_eq!(observation.matches, ids);
            assert!(observation.revision == observations[0].revision);
            #[cfg(feature = "preview")]
            assert_eq!(observation.preview_size, None);
        }
        assert_eq!(picker.query(), "absent");
        assert_eq!(picker.filter().unwrap().len(), 0);
    }
}

#[cfg(feature = "preview")]
mod preview {
    use std::io;

    use super::*;
    use crate::{
        Picker,
        component::{Component, PreviewComponent},
        event::{LayoutEvent, RecvError},
        preview::{PreviewEvent, interior_dimensions},
        rect::{Area, Rect},
        session::{Dimensions, tests::frame_events},
    };

    struct PreviewProbe<'a> {
        observations: &'a mut Vec<(&'static str, Observation)>,
        size: Dimensions,
    }

    impl PreviewProbe<'_> {
        fn observe<T: Send + Sync + 'static>(&mut self, phase: &'static str, cx: &Context<'_, T>) {
            let observation = Observation::new(cx);
            assert_eq!(
                observation.preview_size,
                Some((self.size.width, self.size.height))
            );
            self.observations.push((phase, observation));
        }
    }

    impl Component<()> for PreviewProbe<'_> {
        fn resize(&mut self, area: Area, _: &()) {
            self.size = interior_dimensions(area);
        }

        fn draw<D: Rect>(&mut self, _: &(), _: &mut D) -> io::Result<()> {
            Ok(())
        }
    }

    impl<T: Send + Sync + 'static> PreviewComponent<T> for PreviewProbe<'_> {
        const ENABLED: bool = true;

        fn handle(&mut self, _: PreviewEvent, cx: &Context<'_, T>) {
            self.observe("event", cx);
        }

        fn update(&mut self, cx: &Context<'_, T>, _: Instant) -> bool {
            self.observe("update", cx);
            false
        }

        fn prefetch(&mut self, cx: &Context<'_, T>, _: Instant) {
            self.observe("prefetch", cx);
        }

        fn hide(&mut self) {}
        fn restart(&mut self) {}
    }

    #[test]
    fn preview_observations_follow_dispatch_and_delivered_resizes() {
        let mut picker = PickerOptions::new()
            .sort_results(false)
            .frame_interval(Duration::from_secs(5))
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        picker.filter().unwrap();
        let ids: Vec<_> = Context::new(&picker.core.engine).match_ids().collect();
        let events = [
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Ok(Event::Preview(PreviewEvent::Refresh)),
            Ok(Event::Layout(LayoutEvent::MoveDividerLeft(2))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Ok(Event::MatchList(MatchListEvent::UnqueueAll)),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::UnqueueAll)),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Ok(Event::MatchList(MatchListEvent::UnqueueAll)),
            Ok(Event::Preview(PreviewEvent::Refresh)),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ];
        let mut queued = Vec::new();
        let mut previews = Vec::new();
        let mut terminal = TestTerminal::default();
        picker
            .core
            .pick_impl(
                frame_events(events),
                &mut terminal,
                QueueProbe(&mut queued),
                PreviewProbe {
                    observations: &mut previews,
                    size: Dimensions::default(),
                },
            )
            .unwrap();

        let expected = [
            ("update", 0, (8, 6)),
            ("prefetch", 0, (8, 6)),
            ("event", 1, (8, 6)),
            ("event", 1, (8, 6)),
            ("update", 1, (8, 6)),
            ("prefetch", 1, (10, 6)),
            ("event", 1, (0, 0)),
            ("update", 1, (0, 0)),
            ("prefetch", 1, (10, 6)),
        ];
        assert_eq!(previews.len(), expected.len());
        for ((phase, observation), (expected_phase, rank, size)) in previews.iter().zip(expected) {
            assert_eq!(*phase, expected_phase);
            assert_eq!(observation.rank, Some(rank));
            assert_eq!(observation.hovered, Some(ids[rank as usize]));
            assert_eq!(observation.preview_size, Some(size));
            assert!(observation.revision == previews[0].1.revision);
        }
        assert_eq!(queued.len(), 4);
        for (observation, size) in queued.iter().zip([None, None, Some((0, 0)), Some((10, 6))]) {
            assert_eq!(observation.preview_size, size);
            assert_eq!(observation.hovered, Some(ids[1]));
        }
        assert_eq!(terminal.size_reads, 4);
    }

    #[test]
    fn enabled_preview_can_have_no_drawable_area_or_hovered_item() {
        for size in [(0, 0), (5, 8), (20, 2)] {
            let mut picker: Picker<&str, _> = Picker::new(StrRenderer);
            let mut previews = Vec::new();
            let mut terminal = TestTerminal {
                size: Some(size),
                ..TestTerminal::default()
            };
            picker
                .core
                .pick_impl(
                    TestEvents(|_| Ok(Event::Quit)),
                    &mut terminal,
                    (),
                    PreviewProbe {
                        observations: &mut previews,
                        size: Dimensions::default(),
                    },
                )
                .unwrap();
            assert_eq!(previews.len(), 2);
            for (_, observation) in previews {
                assert_eq!(observation.preview_size, Some((0, 0)));
                assert_eq!(observation.hovered, None);
                assert_eq!(observation.rank, None);
            }
            assert_eq!(terminal.size_reads, 1);
        }
    }
}
