use std::{num::NonZero, time::Duration};

use super::{driver, frame_options, update};
use crate::{
    event::{MatchListEvent, PromptEvent},
    session::{Context, Exit, Interface, ItemId, QueueEvent, QueueStatus},
};

#[derive(Default)]
struct QueueFirst {
    restarts: Vec<Option<ItemId>>,
}

impl Interface<String> for QueueFirst {
    type State = Option<ItemId>;
    type Output<'items> = Option<&'items String>;

    fn init(&mut self, _: &Context<'_, String>) -> Self::State {
        None
    }

    fn is_queued(&self, queued: &Self::State, id: ItemId) -> bool {
        *queued == Some(id)
    }

    fn queue_status(&self, queued: &Self::State) -> Option<QueueStatus> {
        Some(QueueStatus {
            count: u32::from(queued.is_some()),
            limit: NonZero::new(1),
        })
    }

    fn on_queue(
        &mut self,
        queued: &mut Self::State,
        event: QueueEvent<'_, String>,
        cx: &Context<'_, String>,
    ) -> bool {
        let QueueEvent::Toggle(item) = event else {
            return false;
        };
        assert_eq!(cx.hovered().unwrap().id(), item.id());
        if cx.matches().ids().next() != Some(item.id()) {
            return false;
        }
        *queued = (*queued != Some(item.id())).then_some(item.id());
        true
    }

    fn on_restart(&mut self, queued: &mut Self::State, cx: &Context<'_, String>) {
        if let Some(id) = queued.take() {
            assert!(cx.matches().item(id).is_some());
        }
        self.restarts.push(cx.hovered().map(|item| item.id()));
    }

    fn finish<'items>(
        &mut self,
        queued: Self::State,
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Quit => None,
            Exit::Selected(hovered) => queued
                .or(hovered)
                .map(|id| cx.matches().item(id).unwrap().value()),
        }
    }
}

#[test]
fn custom_queue_hooks_control_navigation_and_allow_selection_after_filtering() {
    let mut driver = driver(["alpha", "beta", "gamma"]);
    let mut session = driver.session(QueueFirst::default(), Duration::ZERO);
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
    }
    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.view().context().hovered().unwrap().value(), "beta");
    assert_eq!(frame.view().context().matches().len(), 3);
    assert_eq!(
        frame.view().queue_status(),
        Some(QueueStatus {
            count: 1,
            limit: NonZero::new(1),
        })
    );
    frame.complete();

    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.view().context().matches().is_empty());
    assert_eq!(frame.view().context().rank(), None);
    assert_eq!(frame.view().queue_status().unwrap().count, 1);
    frame.complete();
    let exit = session.events().selection_exit().unwrap();
    assert_eq!(exit, Exit::Selected(None));
    assert_eq!(session.finish(exit).unwrap(), "alpha");

    let mut session = driver.session(QueueFirst::default(), Duration::ZERO);
    assert!(session.events().selection_exit().is_none());
}

#[test]
fn restart_notifies_with_event_time_context_and_replaces_items_without_resetting_the_query() {
    let mut driver = driver(["alpha", "beta"]);
    let old_ids: Vec<_> = driver.matches().ids().collect();
    let old_injector = driver.injector();
    let mut interface = QueueFirst::default();
    let mut session = driver.session(&mut interface, Duration::ZERO);
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.prompt(PromptEvent::Insert('a'));
        drop(batch.restart());
        assert!(batch.selection_exit().is_none());
    }
    let replacement = session.events().restart();
    replacement.push("gamma".to_owned());
    old_injector.push("old generation".to_owned());
    drop((replacement, old_injector));

    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.view().query(), "a");
    assert_eq!(frame.view().queue_status().unwrap().count, 0);
    let view = frame.view();
    let cx = view.context();
    assert_eq!(cx.matches().len(), 1);
    assert!(old_ids.iter().all(|&id| cx.matches().item(id).is_none()));
    assert_eq!(cx.hovered().unwrap().value(), "gamma");
    frame.complete();
    let exit = session.events().selection_exit().unwrap();
    assert_eq!(session.finish(exit).unwrap(), "gamma");
    assert_eq!(interface.restarts, [Some(old_ids[1]), None]);
    assert_eq!(driver.query(), "a");
}
