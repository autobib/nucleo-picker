use std::num::NonZero;

use super::{frame_options, model, update};
use crate::{
    event::{MatchListEvent, PromptEvent},
    interface::Interface,
    session::{Context, Exit, ItemId, QueueEvent, QueueStatus},
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
    let mut model = model(["alpha", "beta", "gamma"]);
    let mut session = model.session(QueueFirst::default());
    {
        let mut batch = session.events();
        assert_eq!(batch.queue_status().unwrap().count, 0);
        batch.match_list(MatchListEvent::ToggleUp(1));
        assert_eq!(batch.queue_status().unwrap().count, 1);
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
    }
    assert_eq!(session.hovered_rank(), Some(1));
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(
        frame.status().queue_status(),
        Some(QueueStatus {
            count: 1,
            limit: NonZero::new(1),
        })
    );
    frame.complete();

    update(&mut session);
    assert!(session.matches().is_empty());
    let exit = {
        let batch = session.events();
        let hovered = batch.hovered_id();
        assert_eq!(hovered, None);
        assert_eq!(batch.queue_status().unwrap().count, 1);
        Exit::Selected(hovered)
    };
    assert_eq!(session.finish(exit).unwrap(), "alpha");
}

#[test]
fn restart_notifies_with_event_time_context_and_replaces_items_without_resetting_the_query() {
    let mut model = model(["alpha", "beta"]);
    let old_hovered = model.matches().get(1).unwrap().id();
    let old_injector = model.injector();
    let mut interface = QueueFirst::default();
    let mut session = model.session(&mut interface);
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.prompt(PromptEvent::Insert('a'));
        drop(batch.restart());
        assert_eq!(batch.hovered_id(), None);
        assert_eq!(batch.queue_status().unwrap().count, 0);
    }
    let replacement = session.events().restart();
    replacement.push("gamma".to_owned());
    old_injector.push("old generation".to_owned());
    drop((replacement, old_injector));

    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.query(), "a");
    assert_eq!(frame.match_status().matched_item_count, 1);
    frame.complete();
    let exit = Exit::Selected(session.events().hovered_id());
    assert_eq!(session.finish(exit).unwrap(), "gamma");
    assert_eq!(interface.restarts, [Some(old_hovered), None]);
    assert_eq!(model.query(), "a");
}
