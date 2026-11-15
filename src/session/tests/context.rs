use std::time::{Duration, Instant};

use super::{TestEvents, TestTerminal};
use crate::{
    PickerOptions,
    component::PreviewComponent,
    event::{Event, MatchListEvent, PromptEvent},
    match_list::SelectionIntoIter,
    render::StrRenderer,
    session::{Context, Exit, Interface, ItemId, Multi, MultiState, QueueEvent},
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
    queued: MultiState,
    exit: Exit,
    cx: Context<'items, T>,
}

impl<'items, T: Send + Sync + 'static> IntoIterator for QueueOutput<'items, T> {
    type Item = &'items T;
    type IntoIter = SelectionIntoIter<'items, T>;

    fn into_iter(self) -> Self::IntoIter {
        Multi::new()
            .finish(self.queued, self.exit, self.cx)
            .into_iter()
    }
}

struct QueueProbe<'a> {
    observations: &'a mut Vec<Observation>,
    initial: Option<ItemId>,
}

impl<'a> QueueProbe<'a> {
    fn new(observations: &'a mut Vec<Observation>) -> Self {
        Self {
            observations,
            initial: None,
        }
    }
}

impl<T: Send + Sync + 'static> Interface<T> for QueueProbe<'_> {
    type State = MultiState;
    type Output<'items> = QueueOutput<'items, T>;

    fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
        let mut state = Multi::new().init(cx);
        if let Some(id) = self.initial {
            let item = cx.matches().item(id).unwrap();
            assert!(Multi::new().on_queue(&mut state, QueueEvent::Toggle(item), cx));
        }
        state
    }

    fn on_restart(&mut self, state: &mut Self::State, cx: &Context<'_, T>) {
        Multi::new().on_restart(state, cx);
    }

    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        <Multi as Interface<T>>::is_queued(&Multi::new(), state, id)
    }

    fn queued_count(&self, state: &Self::State) -> Option<usize> {
        <Multi as Interface<T>>::queued_count(&Multi::new(), state)
    }

    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        self.observations.push(Observation::new(cx));
        Multi::new().on_queue(state, event, cx)
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        self.observations.push(Observation::new(&cx));
        QueueOutput {
            queued: state,
            exit,
            cx,
        }
    }
}

impl<T> PreviewComponent<T, MultiState> for QueueProbe<'_> {}

#[test]
fn initialization_and_restart_observe_the_retained_and_previous_generations() {
    struct Probe<'a, const PREVIEW: bool>(&'a mut Vec<Observation>);

    impl<T: Send + Sync + 'static, const PREVIEW: bool> Interface<T> for Probe<'_, PREVIEW> {
        type State = ();
        type Output<'items> = Option<&'items T>;

        #[cfg(feature = "preview")]
        const HAS_PREVIEW: bool = PREVIEW;

        fn init(&mut self, cx: &Context<'_, T>) {
            self.0.push(Observation::new(cx));
        }

        fn on_restart(&mut self, _: &mut (), cx: &Context<'_, T>) {
            self.0.push(Observation::new(cx));
        }

        fn finish<'items>(
            &mut self,
            (): (),
            exit: Exit,
            cx: Context<'items, T>,
        ) -> Self::Output<'items> {
            crate::Single.finish((), exit, cx)
        }
    }

    impl<T, const PREVIEW: bool> PreviewComponent<T, ()> for Probe<'_, PREVIEW> {}

    fn check<const PREVIEW: bool>() {
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
            picker.push_batch(["theta"]);
            let _injectors = picker.injector_observer(false);
            let navigate = if reversed {
                MatchListEvent::Down(1)
            } else {
                MatchListEvent::Up(1)
            };
            let mut events = [
                Event::MatchList(navigate),
                Event::Prompt(PromptEvent::Insert('x')),
                Event::Restart,
                Event::Restart,
                Event::Quit,
            ]
            .into_iter();
            let mut observations = Vec::new();
            picker
                .core
                .pick_impl(
                    TestEvents(|_| Ok(events.next().unwrap())),
                    &mut TestTerminal::default(),
                    &mut Probe::<PREVIEW>(&mut observations),
                )
                .unwrap();

            let [init, restart, empty_restart] = observations.as_slice() else {
                panic!("expected initialization and two restart observations");
            };
            assert_eq!(init.matches, ids);
            assert_eq!(init.hovered, None);
            assert_eq!(init.rank, None);
            assert_eq!(init.query, "ta");
            #[cfg(feature = "preview")]
            assert_eq!(init.preview_dimensions, PREVIEW.then_some((0, 0)));
            assert_eq!(restart.hovered, Some(ids[1]));
            assert_eq!(restart.rank, Some(1));
            assert_eq!(&restart.matches[..ids.len()], ids.as_slice());
            assert_eq!(restart.query, "ta");
            assert_eq!(empty_restart.hovered, None);
            assert_eq!(empty_restart.rank, None);
            assert!(empty_restart.matches.is_empty());
            #[cfg(feature = "preview")]
            for observation in [restart, empty_restart] {
                assert_eq!(observation.preview_dimensions, PREVIEW.then_some((8, 6)));
            }
        }
    }

    check::<false>();
    #[cfg(feature = "preview")]
    check::<true>();
}

#[cfg(feature = "unstable-backend")]
#[test]
fn custom_output_outlives_a_temporary_picker_ref_and_its_interface_borrows() {
    for preview in [
        false,
        #[cfg(feature = "preview")]
        true,
    ] {
        let mut picker = PickerOptions::new()
            .sort_results(false)
            .query("ta")
            .frame_interval(Duration::from_secs(5))
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta", "delta"].map(str::to_owned));
        picker.filter().unwrap();
        let ids: Vec<_> = picker.matches().ids().collect();
        let selected_ptr = picker.matches().get(0).unwrap().value() as *const String;
        let output = {
            let mut observations = Vec::new();
            let interface = QueueProbe::new(&mut observations);
            let mut events =
                [Event::MatchList(MatchListEvent::ToggleUp(1)), Event::Select].into_iter();
            let events = TestEvents(|_| Ok(events.next().unwrap()));
            let mut terminal = TestTerminal::default();
            #[cfg(feature = "preview")]
            if preview {
                picker
                    .as_interface(crate::WithPreview::new(
                        interface,
                        crate::preview::SyncPreviewer(
                            |_: &String, _: &mut crate::preview::Pane| {},
                        ),
                    ))
                    .pick_with_terminal_io(events, &mut terminal)
                    .unwrap()
            } else {
                picker
                    .as_interface(interface)
                    .pick_with_terminal_io(events, &mut terminal)
                    .unwrap()
            }
            #[cfg(not(feature = "preview"))]
            {
                let _ = preview;
                picker
                    .as_interface(interface)
                    .pick_with_terminal_io(events, &mut terminal)
                    .unwrap()
            }
        };

        assert_eq!(output.exit, Exit::Selected(Some(ids[1])));
        assert_eq!(output.cx.hovered().unwrap().id(), ids[1]);
        assert!(output.cx.matches().ids().eq(ids));
        let mut items = output.into_iter();
        assert_eq!(items.len(), 1);
        let selected = items.next().unwrap();
        assert_eq!(selected, "beta");
        assert_eq!(selected as *const String, selected_ptr);
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
                &mut QueueProbe::new(&mut observations),
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
                    &mut QueueProbe::new(&mut observations),
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
            probe.initial = Some(picker.core.engine.item_id(0));
        }
        let mut events = [Event::Select, Event::Quit].into_iter();
        let output = picker
            .core
            .pick_impl(
                TestEvents(|_| Ok(events.next().unwrap())),
                &mut TestTerminal::default(),
                &mut probe,
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
        queue: QueueProbe<'a>,
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

    impl<T: Send + Sync + 'static> Interface<T> for PreviewProbe<'_> {
        type State = MultiState;
        type Output<'items> = QueueOutput<'items, T>;

        const HAS_PREVIEW: bool = true;

        fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
            self.queue.init(cx)
        }

        fn on_queue(
            &mut self,
            state: &mut Self::State,
            event: QueueEvent<'_, T>,
            cx: &Context<'_, T>,
        ) -> bool {
            self.queue.on_queue(state, event, cx)
        }

        fn finish<'items>(
            &mut self,
            state: Self::State,
            exit: Exit,
            cx: Context<'items, T>,
        ) -> Self::Output<'items> {
            self.queue.finish(state, exit, cx)
        }
    }

    impl<T: Send + Sync + 'static> PreviewComponent<T, MultiState> for PreviewProbe<'_> {
        fn component(
            &mut self,
            _: &mut MultiState,
            _: &crate::PickerChars,
            _: &crate::preview::PreviewConfig,
        ) -> impl Component<()> {
            self
        }

        fn handle(&mut self, _: &mut MultiState, _: PreviewEvent, cx: &Context<'_, T>) {
            self.observe("event", cx);
        }

        fn update(&mut self, _: &mut MultiState, cx: &Context<'_, T>, _: bool, _: Instant) -> bool {
            self.observe("update", cx);
            false
        }

        fn prefetch(&mut self, _: &mut MultiState, cx: &Context<'_, T>, _: Instant) {
            self.observe("prefetch", cx);
        }
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
                &mut PreviewProbe {
                    queue: QueueProbe::new(&mut queued),
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
            let mut queued = Vec::new();
            let mut terminal = TestTerminal {
                size: Some(size),
                ..TestTerminal::default()
            };
            picker
                .core
                .pick_impl(
                    TestEvents(|_| Ok(Event::Quit)),
                    &mut terminal,
                    &mut PreviewProbe {
                        queue: QueueProbe::new(&mut queued),
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
