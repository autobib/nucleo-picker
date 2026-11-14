use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use super::{TestEvents, TestTerminal};
use crate::{
    Picker, PickerOptions,
    component::NoPreview,
    event::{Event, MatchListEvent, PromptEvent},
    match_list::SelectedIndices,
    render::StrRenderer,
    session::{Context, sealed::State},
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
    let queued = State::<&str>::make_queue(&picker.core.multi_defaults);
    picker
        .core
        .pick_impl(receiver, &mut TestTerminal::default(), queued, NoPreview)
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
                .pick_impl(
                    events,
                    &mut TestTerminal::default(),
                    SelectedIndices::default(),
                    NoPreview,
                )
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
        .pick_impl(
            events,
            &mut TestTerminal::default(),
            SelectedIndices::default(),
            NoPreview,
        )
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
