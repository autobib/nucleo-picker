use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use super::{TestEvents, TestTerminal};
use crate::{
    Picker, PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
    render::StrRenderer,
    session::{Context, Multi},
};

fn pick_multi<'a>(
    picker: &'a mut Picker<&'static str, StrRenderer>,
    events: impl IntoIterator<Item = Event>,
) -> crate::Selection<'a, &'static str> {
    let (sender, receiver) = mpsc::channel();
    for event in events {
        sender.send(event).unwrap();
    }
    drop(sender);
    let mut interface = picker.core.multi_defaults;
    picker
        .core
        .pick_impl(receiver, &mut TestTerminal::default(), &mut interface)
        .unwrap()
}

#[test]
fn multi_selection_falls_back_to_the_buffered_filtered_item() {
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .query("ta")
        .picker(StrRenderer);
    picker.push_batch(["alpha", "beta", "delta"]);
    picker.filter().unwrap();

    let selected = pick_multi(
        &mut picker,
        [Event::MatchList(MatchListEvent::Up(1)), Event::Select],
    );
    assert!(selected.into_iter().copied().eq(["delta"]));
}

#[test]
fn queued_items_survive_filtering_and_are_discarded_on_quit() {
    for (query, matched) in [("^gamma$", 1), ("absent", 0)] {
        for exit in [Event::Select, Event::Quit, Event::QuitPromptEmpty] {
            let mut picker = PickerOptions::new()
                .sort_results(false)
                .frame_interval(Duration::ZERO)
                .picker(StrRenderer);
            picker.push_batch(["alpha", "beta", "gamma"]);
            picker.filter().unwrap();
            let statuses = picker.status_observer();
            let selects = matches!(exit, Event::Select);
            let mut exit = Some(exit);
            let mut quitting_empty = false;
            let mut initial = [
                Event::MatchList(MatchListEvent::Up(1)),
                Event::MatchList(MatchListEvent::ToggleUp(1)),
                Event::Prompt(PromptEvent::Reset(query.to_owned())),
            ]
            .into_iter();
            let start = Instant::now();
            let events = TestEvents(|_| {
                assert!(start.elapsed() < Duration::from_secs(5));
                if let Some(event) = initial.next() {
                    return Ok(event);
                }
                if quitting_empty {
                    return Ok(exit.take().unwrap());
                }
                if let Ok(status) = statuses.try_recv() {
                    assert_eq!(status.selected_item_count, 1);
                    if !status.matching && status.matched_item_count == matched {
                        if matches!(exit, Some(Event::QuitPromptEmpty)) {
                            quitting_empty = true;
                            return Ok(Event::Prompt(PromptEvent::Reset(String::new())));
                        }
                        return Ok(exit.take().unwrap());
                    }
                }
                Ok(Event::Status { id: 0 })
            });
            let selected = picker
                .core
                .pick_impl(events, &mut TestTerminal::default(), &mut Multi::new())
                .unwrap();
            let expected: &[_] = if selects { &["beta"] } else { &[] };
            assert!(selected.into_iter().copied().eq(expected.iter().copied()));
        }
    }
}

#[test]
fn restart_clears_queued_items_before_reusing_indices() {
    let mut picker = PickerOptions::new()
        .frame_interval(Duration::ZERO)
        .picker(StrRenderer);
    picker.push_batch(["original"]);
    picker.filter().unwrap();
    let original = picker.core.engine.item_id(0);
    let injectors = picker.injector_observer(false);
    let statuses = picker.status_observer();
    let mut initial = [
        Event::MatchList(MatchListEvent::ToggleUp(0)),
        Event::Restart,
    ]
    .into_iter();
    let mut injected = false;
    let start = Instant::now();
    let events = TestEvents(|_| {
        assert!(start.elapsed() < Duration::from_secs(5));
        if let Some(event) = initial.next() {
            return Ok(event);
        }
        if !injected {
            injectors.try_recv().unwrap().push_batch(["replacement"]);
            injected = true;
        }
        if let Ok(status) = statuses.try_recv() {
            assert_eq!(status.selected_item_count, 0);
            if !status.matching && status.matched_item_count == 1 {
                return Ok(Event::Select);
            }
        }
        Ok(Event::Status { id: 0 })
    });
    let selected = picker
        .core
        .pick_impl(events, &mut TestTerminal::default(), &mut Multi::new())
        .unwrap();
    assert!(selected.into_iter().copied().eq(["replacement"]));

    let cx = Context::new(&picker.core.engine);
    assert!(cx.matches().item(original).is_none());
    let replacement = cx.matches().ids().next().unwrap();
    assert_ne!(replacement, original);
    assert_eq!(
        cx.matches().item(replacement).unwrap().value(),
        &"replacement"
    );
}

#[test]
fn custom_queue_hooks_control_navigation_markers_status_and_output() {
    use super::frame_events;
    use crate::{
        event::RecvError,
        session::{Exit, Interface, ItemId, QueueEvent},
    };
    use std::num::NonZero;

    struct FirstOnly;

    struct State {
        first: ItemId,
        queued: bool,
        limit: NonZero<u32>,
        ended: bool,
    }

    impl Interface<&'static str> for FirstOnly {
        type State = State;
        type Output<'items> = Option<&'items &'static str>;

        fn init(&mut self, cx: &Context<'_, &'static str>) -> Self::State {
            State {
                first: cx.matches().ids().next().unwrap(),
                queued: false,
                limit: NonZero::new(1).unwrap(),
                ended: false,
            }
        }

        fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
            state.queued && id == state.first
        }

        fn queued_count(&self, state: &Self::State) -> Option<usize> {
            Some(usize::from(state.queued))
        }

        fn queue_limit(&self, state: &Self::State) -> Option<NonZero<u32>> {
            Some(state.limit)
        }

        fn on_queue(
            &mut self,
            state: &mut Self::State,
            event: QueueEvent<'_, &'static str>,
            cx: &Context<'_, &'static str>,
        ) -> bool {
            if let QueueEvent::Toggle(item) = event {
                assert_eq!(cx.hovered().unwrap().id(), item.id());
                if item.id() == state.first {
                    state.queued = !state.queued;
                    return true;
                }
            }
            false
        }

        fn on_restart(&mut self, state: &mut Self::State, cx: &Context<'_, &'static str>) {
            assert!(state.queued);
            assert!(cx.matches().item(state.first).is_some());
            assert_eq!(cx.rank(), Some(1));
            state.queued = false;
        }

        fn on_end(&mut self, state: &mut Self::State) {
            state.ended = true;
        }

        fn finish<'items>(
            &mut self,
            state: Self::State,
            exit: Exit,
            cx: Context<'items, &'static str>,
        ) -> Self::Output<'items> {
            assert!(state.ended);
            match exit {
                Exit::Quit => {
                    assert!(!state.queued);
                    None
                }
                Exit::Selected(hovered) => {
                    assert!(state.queued);
                    assert_ne!(hovered, Some(state.first));
                    Some(cx.matches().item(state.first).unwrap().value())
                }
            }
        }
    }

    for preview in [
        false,
        #[cfg(feature = "preview")]
        true,
    ] {
        for restart in [false, true] {
            let mut picker = PickerOptions::new()
                .sort_results(false)
                .frame_interval(Duration::from_secs(5))
                .picker(StrRenderer);
            picker.push_batch(["alpha", "beta", "gamma"]);
            picker.filter().unwrap();
            let statuses = picker.status_observer();
            let _injectors = picker.injector_observer(false);
            let marker = picker.core.chars.queued;
            let events = frame_events([
                Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                Ok(Event::Status { id: 7 }),
                Err(RecvError::Timeout),
                Ok(if restart {
                    Event::Restart
                } else {
                    Event::Select
                }),
                Ok(Event::Quit),
            ]);
            let mut terminal = TestTerminal {
                size: Some((40, 8)),
                ..TestTerminal::default()
            };
            #[cfg(feature = "preview")]
            let output = if preview {
                picker
                    .core
                    .pick_impl(
                        events,
                        &mut terminal,
                        &mut crate::WithPreview::new(
                            FirstOnly,
                            crate::preview::SyncPreviewer(
                                |_: &&str, _: &mut crate::preview::Pane| {},
                            ),
                        ),
                    )
                    .unwrap()
            } else {
                picker
                    .core
                    .pick_impl(events, &mut terminal, &mut FirstOnly)
                    .unwrap()
            };
            #[cfg(not(feature = "preview"))]
            let output = {
                let _ = preview;
                picker
                    .core
                    .pick_impl(events, &mut terminal, &mut FirstOnly)
                    .unwrap()
            };
            assert_eq!(output.copied(), (!restart).then_some("alpha"));
            let status = statuses.try_recv().unwrap();
            assert_eq!(status.id, 7);
            assert_eq!(status.rank, Some(1));
            assert_eq!(status.selected_item_count, 1);
            let rendered = String::from_utf8(terminal.output).unwrap();
            assert!(rendered.contains("(1/1)"));
            assert_eq!(rendered.matches(marker).count(), 1);
        }
    }
}
