use std::time::{Duration, Instant};

use super::{Select, frame_options, model, update};
use crate::{
    event::{MatchListEvent, PromptEvent},
    interface::Interface,
    render::StrRenderer,
    session::{Components, Context, Exit, ItemId, Session},
};

struct Update {
    matches_changed: bool,
    hovered: Option<ItemId>,
    matches: Vec<ItemId>,
    active_components: Components,
    deadline: Instant,
    completed: bool,
}

struct Probe<'a, const PREVIEW: bool>(&'a mut Vec<Update>, Components, Components);

impl<const PREVIEW: bool> Interface<String> for Probe<'_, PREVIEW> {
    type State = ();
    type Output<'items> = Option<&'items String>;

    const PREVIEW: bool = PREVIEW;

    fn init(&mut self, _: &Context<'_, String>) {}

    fn before_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, String>,
        matches_changed: bool,
        deadline: Instant,
    ) -> Components {
        self.0.push(Update {
            matches_changed,
            hovered: cx.hovered().map(|item| item.id()),
            matches: cx.matches().ids().collect(),
            active_components: cx.active_components(),
            deadline,
            completed: false,
        });
        self.1
    }

    fn after_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, String>,
        deadline: Instant,
    ) -> Components {
        let last = self.0.last_mut().unwrap();
        assert!(!std::mem::replace(&mut last.completed, true));
        assert_eq!(last.deadline, deadline);
        assert_eq!(last.hovered, cx.hovered().map(|item| item.id()));
        assert!(cx.matches().ids().eq(last.matches.iter().copied()));
        assert_eq!(last.active_components, cx.active_components());
        self.2
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        Select.finish((), exit, cx)
    }
}

fn frame<I: Interface<String>>(session: &mut Session<'_, String, StrRenderer, I>) {
    session.prepare_frame(frame_options()).complete();
}

#[test]
fn idle_and_navigation_frames_run_both_hooks_without_match_notifications() {
    let mut model = model(["alpha", "beta"]);
    let ids: Vec<_> = model.matches().ids().collect();
    let mut updates = Vec::new();
    let mut session = model.session(Probe::<false>(
        &mut updates,
        Components::NONE,
        Components::NONE,
    ));
    frame(&mut session);
    frame(&mut session);
    session.events().match_list(MatchListEvent::Up(1));
    frame(&mut session);
    session.events().match_list(MatchListEvent::Reset);
    frame(&mut session);
    drop(session);

    assert_eq!(updates.len(), 4);
    for (update, (changed, rank)) in
        updates
            .iter()
            .zip([(true, 0), (false, 0), (false, 1), (false, 0)])
    {
        assert_eq!(update.matches_changed, changed);
        assert_eq!(update.hovered, Some(ids[rank]));
        assert_eq!(update.matches, ids);
        assert_eq!(
            update.active_components,
            Components {
                preview: false,
                ..Components::ALL
            }
        );
        assert!(update.completed);
    }
}

#[test]
fn query_batches_notify_only_after_publication_and_keep_changes_until_preparation() {
    let mut model = model(["alpha", "beta"]);
    let beta = model.matches().get(1).unwrap().id();
    let mut updates = Vec::new();
    let mut session = model.session(Probe::<false>(
        &mut updates,
        Components::NONE,
        Components::NONE,
    ));
    frame(&mut session);
    session
        .events()
        .prompt(PromptEvent::Reset("beta".to_owned()));
    session.events().prompt(PromptEvent::Left(1));
    frame(&mut session);
    update(&mut session);
    session.update_matches(Duration::ZERO);
    frame(&mut session);
    frame(&mut session);
    drop(session);

    assert!(
        updates
            .iter()
            .map(|update| (update.matches_changed, update.matches.len()))
            .eq([(true, 2), (false, 2), (true, 1), (false, 1),])
    );
    assert_eq!(updates[2].hovered, Some(beta));
    assert!(updates.iter().all(|update| update.completed));
    assert_eq!(model.query(), "beta");
}

#[test]
fn hidden_publications_and_repeated_restarts_are_reported_once_per_frame() {
    let mut model = model(["alpha", "beta"]);
    let injector = model.injector();
    let mut updates = Vec::new();
    let mut session = model.session(Probe::<true>(
        &mut updates,
        Components::NONE,
        Components::NONE,
    ));
    frame(&mut session);
    session.events().set_active_components(Components {
        preview: false,
        ..Components::ALL
    });
    frame(&mut session);
    injector.push("gamma".to_owned());
    drop(injector);
    update(&mut session);
    frame(&mut session);
    session.events().set_active_components(Components::ALL);
    frame(&mut session);
    {
        let mut batch = session.events();
        drop(batch.restart());
        drop(batch.restart());
    }
    update(&mut session);
    frame(&mut session);
    frame(&mut session);
    drop(session.events().restart());
    update(&mut session);
    frame(&mut session);
    drop(session);

    assert!(
        updates
            .iter()
            .map(|update| (
                update.matches_changed,
                update.active_components.preview,
                update.matches.len(),
            ))
            .eq([
                (true, true, 2),
                (false, false, 2),
                (true, false, 3),
                (false, true, 3),
                (true, true, 0),
                (false, true, 0),
                (true, true, 0),
            ])
    );
    assert!(updates.iter().all(|update| update.completed));
}
