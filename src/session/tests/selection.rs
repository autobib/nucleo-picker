use std::num::NonZero;

use super::{driver, frame_options, update};
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
    let mut session = driver.session(QueueFirst::default());
    {
        let mut batch = session.events();
        assert_eq!(batch.queue_status().unwrap().count, 0);
        batch.match_list(MatchListEvent::ToggleUp(1));
        assert_eq!(batch.queue_status().unwrap().count, 1);
        batch.match_list(MatchListEvent::ToggleUp(1));
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
    }
    assert_eq!(session.query(), "absent");
    assert_eq!(session.rank(), Some(1));
    assert_eq!(
        session.queue_status(),
        Some(QueueStatus {
            count: 1,
            limit: NonZero::new(1),
        })
    );
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.prompt().query(), "absent");
    let list = frame.match_list();
    assert_eq!(list.rank(), Some(1));
    let matches = list.matches();
    assert!(
        matches
            .iter()
            .map(|item| item.value().as_str())
            .eq(["alpha", "beta", "gamma"])
    );
    assert_eq!(
        matches.get(list.rank().unwrap() as usize).unwrap().value(),
        "beta"
    );
    assert_eq!(frame.status().match_status().matched_item_count, 3);
    assert_eq!(
        frame.status().queue_status(),
        Some(QueueStatus {
            count: 1,
            limit: NonZero::new(1),
        })
    );
    frame.complete();

    update(&mut session);
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.status().match_status().matched_item_count, 0);
    assert_eq!(frame.match_list().rank(), None);
    assert!(frame.match_list().matches().is_empty());
    assert_eq!(frame.status().queue_status().unwrap().count, 1);
    frame.complete();
    let exit = {
        let batch = session.events();
        let hovered = batch.hovered_id();
        assert_eq!(hovered, None);
        assert_eq!(batch.queue_status().unwrap().count, 1);
        Exit::Selected(hovered)
    };
    assert_eq!(session.finish(exit).unwrap(), "alpha");

    let mut session = driver.session(QueueFirst::default());
    let batch = session.events();
    assert_eq!(batch.hovered_id(), None);
    assert_eq!(batch.queue_status().unwrap().count, 0);
}

#[test]
fn restart_notifies_with_event_time_context_and_replaces_items_without_resetting_the_query() {
    let mut driver = driver(["alpha", "beta"]);
    let old_ids: Vec<_> = driver.matches().ids().collect();
    let old_injector = driver.injector();
    let mut interface = QueueFirst::default();
    let mut session = driver.session(&mut interface);
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

    assert_eq!(
        session.queue_status(),
        Some(QueueStatus {
            count: 0,
            limit: NonZero::new(1),
        })
    );
    update(&mut session);
    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.query(), "a");
    assert_eq!(frame.queue_status().unwrap().count, 0);
    assert_eq!(frame.match_status().matched_item_count, 1);
    assert_eq!(frame.rank(), Some(0));
    frame.complete();
    let exit = Exit::Selected(session.events().hovered_id());
    assert_eq!(session.finish(exit).unwrap(), "gamma");
    assert!(
        old_ids
            .iter()
            .all(|&id| driver.matches().item(id).is_none())
    );
    assert_eq!(interface.restarts, [Some(old_ids[1]), None]);
    assert_eq!(driver.query(), "a");
}
