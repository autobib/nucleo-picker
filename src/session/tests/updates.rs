use std::time::{Duration, Instant};

use super::{Select, driver, frame_options, update};
use crate::{
    event::{MatchListEvent, PromptEvent},
    render::StrRenderer,
    session::{Context, Exit, Geometry, Interface, ItemId, Redraw, Session},
};

struct Update {
    matches_changed: bool,
    hovered: Option<ItemId>,
    matches: Vec<ItemId>,
    preview_enabled: bool,
    deadline: Instant,
    completed: bool,
}

struct Probe<'a, const PREVIEW: bool>(&'a mut Vec<Update>, Redraw);

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
    ) -> Redraw {
        self.0.push(Update {
            matches_changed,
            hovered: cx.hovered().map(|item| item.id()),
            matches: cx.matches().ids().collect(),
            preview_enabled: cx.preview_enabled(),
            deadline,
            completed: false,
        });
        self.1
    }

    fn after_frame(&mut self, _: &mut (), cx: &Context<'_, String>, deadline: Instant) -> Redraw {
        let last = self.0.last_mut().unwrap();
        assert!(!std::mem::replace(&mut last.completed, true));
        assert_eq!(last.deadline, deadline);
        assert_eq!(last.hovered, cx.hovered().map(|item| item.id()));
        assert!(cx.matches().ids().eq(last.matches.iter().copied()));
        assert_eq!(last.preview_enabled, cx.preview_enabled());
        self.1
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
    let mut frame = session.prepare_frame(frame_options());
    frame.resize(Geometry::default());
    frame.complete();
}

#[test]
fn idle_and_navigation_frames_run_both_hooks_without_match_notifications() {
    let mut driver = driver(["alpha", "beta"]);
    let ids: Vec<_> = driver.matches().ids().collect();
    let mut updates = Vec::new();
    let mut session = driver.session(Probe::<false>(&mut updates, Redraw::NONE), Duration::ZERO);
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
        assert!(!update.preview_enabled);
        assert!(update.completed);
    }
}

#[test]
fn query_batches_notify_only_after_publication_and_keep_changes_until_preparation() {
    let mut driver = driver(["alpha", "beta"]);
    let beta = driver.matches().get(1).unwrap().id();
    let mut updates = Vec::new();
    let mut session = driver.session(Probe::<false>(&mut updates, Redraw::NONE), Duration::ZERO);
    frame(&mut session);
    session
        .events()
        .prompt(PromptEvent::Reset("beta".to_owned()));
    session.events().prompt(PromptEvent::Left(1));
    frame(&mut session);
    update(&mut session);
    session.update(Duration::ZERO);
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
    assert_eq!(driver.query(), "beta");
}

#[test]
fn hidden_publications_and_repeated_restarts_are_reported_once_per_frame() {
    let mut driver = driver(["alpha", "beta"]);
    let injector = driver.injector();
    let mut updates = Vec::new();
    let mut session = driver.session(Probe::<true>(&mut updates, Redraw::NONE), Duration::ZERO);
    frame(&mut session);
    session.events().set_preview_enabled(false);
    frame(&mut session);
    injector.push("gamma".to_owned());
    drop(injector);
    update(&mut session);
    frame(&mut session);
    session.events().set_preview_enabled(true);
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
                update.preview_enabled,
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

#[test]
fn preview_wrappers_forward_frame_hooks_and_combine_invalidations() {
    fn check<I: Interface<String>>(interface: I) {
        let mut driver = driver(["alpha", "beta"]);
        let mut session = driver.session(interface, Duration::ZERO);
        frame(&mut session);
        session.events().match_list(MatchListEvent::Up(1));
        let navigation = session.prepare_frame(frame_options());
        assert_eq!(navigation.redraw(), Redraw::QUEUE | Redraw::PREVIEW);
        navigation.complete();
        session.events().set_preview_enabled(false);
        frame(&mut session);
        let hidden = session.prepare_frame(frame_options());
        assert_eq!(hidden.redraw(), Redraw::QUEUE);
        hidden.complete();
        drop(session.events().restart());
        update(&mut session);
        frame(&mut session);
        session.events().set_preview_enabled(true);
        frame(&mut session);
    }

    for cached in [
        false,
        #[cfg(feature = "preview-cache")]
        true,
    ] {
        let mut updates = Vec::new();
        let mut generated = 0;
        let probe = Probe::<false>(&mut updates, Redraw::QUEUE);
        let generate = |_: &String, _: &mut crate::preview::Pane| generated += 1;
        match cached {
            #[cfg(feature = "preview-cache")]
            true => check(crate::CachedPreview::new(
                probe,
                crate::preview::SyncPreviewer(generate),
            )),
            _ => check(crate::UncachedPreview::new(probe, generate)),
        }
        assert!(
            updates
                .iter()
                .map(|update| (update.matches_changed, update.preview_enabled))
                .eq([
                    (true, true),
                    (false, true),
                    (false, false),
                    (false, false),
                    (true, false),
                    (false, true),
                ])
        );
        assert!(updates.iter().all(|update| update.completed));
        assert_eq!(generated, 2);
    }
}
