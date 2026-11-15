use std::time::{Duration, Instant};

use super::{TestEvents, TestTerminal};
use crate::{
    PickerOptions,
    component::NoPreview,
    event::{Event, MatchListEvent, PromptEvent},
    match_list::{QueueEvent, Queued, SelectedIndices, SelectionIntoIter},
    render::StrRenderer,
    session::{Context, Exit, ItemId},
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
    assert!(cx.preview_dimensions().is_none());
    picker.push_batch(["alpha", "beta", "delta"]);
    assert!(Context::new(&picker.core.engine).matches().is_empty());
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine);
    assert!(cx.hovered().is_none());
    assert_eq!(cx.rank(), None);
    let cx = cx.with_rank(Some(1));
    let hovered = cx.hovered().unwrap();
    assert_eq!(cx.rank(), Some(1));
    assert_eq!(cx.matches().ids().nth(1), Some(hovered.id()));
    assert_eq!(hovered.value(), &"delta");
    assert_eq!(hovered.id(), picker.core.engine.item_id(2));
    let ids: Vec<_> = cx.matches().ids().collect();
    picker.filter().unwrap();
    assert!(
        Context::new(&picker.core.engine)
            .matches()
            .ids()
            .eq(ids.iter().copied())
    );

    picker.update_query("delta");
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert_eq!(cx.query(), "delta");
    assert_eq!(cx.hovered().unwrap().value(), &"beta");
    assert_eq!(cx.matches().len(), 2);
    assert!(cx.matches().ids().eq(ids));

    let start = Instant::now();
    loop {
        let status = picker.update_matches(Duration::from_millis(5));
        if !status.matching {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert_eq!(cx.hovered().unwrap().value(), &"delta");
    let old_id = cx.hovered().unwrap().id();

    picker.restart();
    let cx = Context::new(&picker.core.engine);
    assert_eq!(cx.matches().len(), 0);
    assert!(cx.matches().item(old_id).is_none());
    picker.restart();
    picker.push_batch(["replacement"]);
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert_eq!(cx.hovered().unwrap().value(), &"replacement");

    picker.update_query("absent");
    picker.filter().unwrap();
    let cx = Context::new(&picker.core.engine).with_rank(Some(0));
    assert!(cx.hovered().is_none());
    assert_eq!(cx.rank(), None);
}

#[test]
fn shared_matches_and_item_borrows_outlive_the_context() {
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .query("ta")
        .picker(StrRenderer);
    picker.push_batch(["alpha", "beta", "delta"].map(str::to_owned));
    picker.filter().unwrap();

    let (matches, hovered, mut items, mut ids) = {
        let cx = Context::new(&picker.core.engine).with_rank(Some(1));
        let matches = cx.matches();
        (
            matches,
            cx.hovered().unwrap(),
            matches.iter(),
            matches.ids(),
        )
    };
    let expected = picker.matches().get(1).unwrap();
    assert_eq!(expected.value(), "delta");
    assert_eq!(hovered.id(), expected.id());
    assert_eq!(matches.get(1).unwrap().id(), expected.id());
    assert_eq!(items.nth(1).unwrap().id(), expected.id());
    assert_eq!(ids.nth(1), Some(expected.id()));
    assert!(std::ptr::eq(hovered.value(), expected.value()));
}

struct Observation {
    hovered: Option<ItemId>,
    rank: Option<u32>,
    query: String,
    matches: Vec<ItemId>,
    #[cfg(feature = "preview")]
    preview_dimensions: Option<(u16, u16)>,
}

impl Observation {
    fn new<T: Send + Sync + 'static>(cx: &Context<'_, T>) -> Self {
        Self {
            hovered: cx.hovered().map(|item| item.id()),
            rank: cx.rank(),
            query: cx.query().to_owned(),
            matches: cx.matches().ids().collect(),
            #[cfg(feature = "preview")]
            preview_dimensions: cx
                .preview_dimensions()
                .map(|size| (size.width, size.height)),
        }
    }
}

struct QueueOutput<'items, T> {
    queued: SelectedIndices,
    exit: Exit,
    cx: Context<'items, T>,
}

impl<'items, T: Send + Sync + 'static> IntoIterator for QueueOutput<'items, T> {
    type Item = &'items T;
    type IntoIter = SelectionIntoIter<'items, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.queued.on_select(self.exit, self.cx).into_iter()
    }
}

struct QueueProbe<'a> {
    observations: &'a mut Vec<Observation>,
    queued: SelectedIndices,
}

impl<'a> QueueProbe<'a> {
    fn new(observations: &'a mut Vec<Observation>) -> Self {
        Self {
            observations,
            queued: SelectedIndices::default(),
        }
    }
}

impl Queued for QueueProbe<'_> {
    type Output<'items, T: Send + Sync + 'static> = QueueOutput<'items, T>;

    fn is_queued(&self, id: ItemId) -> bool {
        self.queued.is_queued(id)
    }

    fn count(&self) -> Option<usize> {
        self.queued.count()
    }

    fn on_queue<T: Send + Sync + 'static>(
        &mut self,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        self.observations.push(Observation::new(cx));
        self.queued.on_queue(event, cx)
    }

    fn on_select<'items, T: Send + Sync + 'static>(
        self,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items, T> {
        self.observations.push(Observation::new(&cx));
        QueueOutput {
            queued: self.queued,
            exit,
            cx,
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
        let ids: Vec<_> = Context::new(&picker.core.engine).matches().ids().collect();
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
                QueueProbe::new(&mut observations),
                NoPreview,
            )
            .unwrap();
        assert_eq!(selected.exit, Exit::Selected(Some(ids[1])));
        assert!(selected.into_iter().copied().eq(["delta"]));
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
            #[cfg(feature = "preview")]
            assert_eq!(observation.preview_dimensions, None);
        }
        assert_eq!(picker.query(), "absent");
        assert_eq!(picker.filter().unwrap().len(), 0);
    }
}

#[test]
fn queued_selection_retains_the_buffered_hover_and_exit_context() {
    for reversed in [false, true] {
        let mut picker = PickerOptions::new()
            .sort_results(false)
            .query("ta")
            .reversed(reversed)
            .frame_interval(Duration::from_secs(5))
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta", "delta"]);
        picker.filter().unwrap();
        let ids: Vec<_> = picker.matches().ids().collect();
        let toggle = if reversed {
            MatchListEvent::ToggleDown(1)
        } else {
            MatchListEvent::ToggleUp(1)
        };
        let output = {
            let mut observations = Vec::new();
            let mut events = [
                Event::MatchList(toggle),
                Event::Prompt(PromptEvent::Reset("absent".to_owned())),
                Event::Select,
            ]
            .into_iter();
            picker
                .core
                .pick_impl(
                    TestEvents(|_| Ok(events.next().unwrap())),
                    &mut TestTerminal::default(),
                    QueueProbe::new(&mut observations),
                    NoPreview,
                )
                .unwrap()
        };

        assert_eq!(output.exit, Exit::Selected(Some(ids[1])));
        assert_eq!(output.cx.hovered().unwrap().id(), ids[1]);
        assert_eq!(output.cx.rank(), Some(1));
        assert_eq!(output.cx.query(), "absent");
        assert!(output.cx.matches().ids().eq(ids));
        assert!(output.into_iter().copied().eq(["beta"]));
        assert_eq!(picker.query(), "absent");
        assert!(picker.filter().unwrap().is_empty());
    }
}

#[test]
fn selection_without_a_hovered_item_requires_queued_items() {
    for has_queued in [false, true] {
        let mut picker = PickerOptions::new().query("absent").picker(StrRenderer);
        picker.push_batch(["alpha"]);
        picker.filter().unwrap();
        let mut observations = Vec::new();
        let mut probe = QueueProbe::new(&mut observations);
        if has_queued {
            let cx = Context::new(&picker.core.engine);
            let item = cx.matches().item(picker.core.engine.item_id(0)).unwrap();
            assert!(probe.on_queue(QueueEvent::Toggle(item), &cx));
        }
        let mut events = [Event::Select, Event::Quit].into_iter();
        let output = picker
            .core
            .pick_impl(
                TestEvents(|_| Ok(events.next().unwrap())),
                &mut TestTerminal::default(),
                probe,
                NoPreview,
            )
            .unwrap();

        assert!(output.cx.hovered().is_none());
        assert_eq!(output.cx.rank(), None);
        assert_eq!(output.cx.query(), "absent");
        assert!(output.cx.matches().is_empty());
        if has_queued {
            assert_eq!(output.exit, Exit::Selected(None));
            assert!(output.into_iter().copied().eq(["alpha"]));
        } else {
            assert_eq!(output.exit, Exit::Quit);
            assert_eq!(output.into_iter().len(), 0);
        }
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
            if let Some(size) = observation.preview_dimensions {
                assert_eq!(size, (self.size.width, self.size.height));
            } else {
                assert_eq!(phase, "update");
            }
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

        fn update(&mut self, cx: &Context<'_, T>, _: bool, _: Instant) -> bool {
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
        let ids: Vec<_> = Context::new(&picker.core.engine).matches().ids().collect();
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
                QueueProbe::new(&mut queued),
                PreviewProbe {
                    observations: &mut previews,
                    size: Dimensions::default(),
                },
            )
            .unwrap();

        let expected = [
            ("update", 0, Some((8, 6))),
            ("prefetch", 0, Some((8, 6))),
            ("event", 1, Some((8, 6))),
            ("event", 1, Some((8, 6))),
            ("update", 1, Some((8, 6))),
            ("prefetch", 1, Some((10, 6))),
            ("update", 1, None),
            ("event", 1, Some((0, 0))),
            ("update", 1, Some((0, 0))),
            ("prefetch", 1, Some((10, 6))),
        ];
        assert_eq!(previews.len(), expected.len());
        for ((phase, observation), (expected_phase, rank, size)) in previews.iter().zip(expected) {
            assert_eq!(*phase, expected_phase);
            assert_eq!(observation.rank, Some(rank));
            assert_eq!(observation.hovered, Some(ids[rank as usize]));
            assert_eq!(observation.preview_dimensions, size);
        }
        assert_eq!(queued.len(), 4);
        for (observation, size) in queued.iter().zip([None, None, Some((0, 0)), Some((10, 6))]) {
            assert_eq!(observation.preview_dimensions, size);
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
                assert_eq!(observation.preview_dimensions, Some((0, 0)));
                assert_eq!(observation.hovered, None);
                assert_eq!(observation.rank, None);
            }
            assert_eq!(terminal.size_reads, 1);
        }
    }
}
