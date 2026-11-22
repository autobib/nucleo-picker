use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use super::{Select, driver, frame_options, update};
use crate::{
    component::tests::TestSurface,
    event::{MatchListEvent, PromptEvent},
    interface::Interface,
    preview::{BoundaryChars, Content, PreviewEvent},
    rect::{Area, ClearState, CrosstermTarget},
    render::StrRenderer,
    session::{Components, Context, Dimensions, DriverOptions, Exit, Position},
};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Init(Option<String>, bool),
    Before(bool, bool),
    After,
    Resize(Option<Dimensions>),
    Preview(Option<String>, Option<Dimensions>, bool),
    End,
    Finish,
}

#[derive(Default)]
struct Probe {
    calls: Vec<Call>,
    after_redraw: std::vec::IntoIter<Components>,
    size: Option<Dimensions>,
}

impl Probe {
    fn hook_counts(&self) -> (usize, usize) {
        (
            self.calls
                .iter()
                .filter(|call| matches!(call, Call::Before(..)))
                .count(),
            self.calls
                .iter()
                .filter(|call| **call == Call::After)
                .count(),
        )
    }
}

impl Interface<String> for Probe {
    type State = ();
    type Output<'items> = Option<&'items String>;

    const PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, String>) {
        self.calls.push(Call::Init(
            cx.hovered().map(|item| item.value().clone()),
            cx.preview_enabled(),
        ));
    }

    fn before_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, String>,
        changed: bool,
        _: Instant,
    ) -> Components {
        self.calls.push(Call::Before(changed, cx.preview_enabled()));
        Components::NONE
    }

    fn after_frame(&mut self, _: &mut (), _: &Context<'_, String>, _: Instant) -> Components {
        self.calls.push(Call::After);
        self.after_redraw.next().unwrap_or(Components::NONE)
    }

    fn preview_resize(&mut self, _: &mut (), size: Option<Dimensions>) {
        self.calls.push(Call::Resize(size));
        self.size = size;
    }

    fn preview_event(
        &mut self,
        _: &mut (),
        _: PreviewEvent,
        cx: &Context<'_, String>,
    ) -> Components {
        self.calls.push(Call::Preview(
            cx.hovered().map(|item| item.value().clone()),
            self.size,
            cx.preview_enabled(),
        ));
        Components::PREVIEW
    }

    fn on_end(&mut self, _: &mut ()) {
        self.calls.push(Call::End);
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        self.calls.push(Call::Finish);
        Select.finish((), exit, cx)
    }
}

#[test]
fn host_presentation_changes_do_not_invalidate_session_content() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    session.prepare_frame(frame_options()).complete();
    for (width, height) in [(20, 7), (13, 4), (0, 4), (13, 0), (20, 7)] {
        let mut frame = session.prepare_frame(frame_options());
        assert!(!frame.changed());
        let size = Dimensions { width, height };
        frame.prompt().resize(size);
        frame.match_list().resize(size);
        frame.status().resize(size);
        frame.preview().border(BoundaryChars::ascii()).resize(size);
        assert!(!frame.changed());
        let mut surface = TestSurface::new(width, height);
        frame.prompt().draw(&mut surface).unwrap();
        frame.match_list().draw(&mut surface).unwrap();
        frame.status().draw(&mut surface).unwrap();
        frame.preview().draw(&mut surface).unwrap();
        assert!(!frame.changed());
        assert!(frame.redraw().is_none());
        frame.complete();
    }
    session.events().preview(PreviewEvent::Refresh);
    let mut frame = session.prepare_frame(frame_options());
    frame.preview().resize(Dimensions::default());
    assert_eq!(frame.redraw(), Components::PREVIEW);
    frame.complete();
}

#[test]
fn construction_leaves_unpublished_items_for_an_explicit_update() {
    for (query, matched) in [("", 2), ("^alpha$", 1)] {
        let mut probe = Probe::default();
        let mut driver = DriverOptions::new()
            .sort_results(false)
            .query(query)
            .driver(StrRenderer);
        driver.push_batch(["alpha", "beta"].map(str::to_owned));
        let mut session = driver.session(&mut probe);
        let initial = session.match_status();
        assert_eq!(initial.item_count, 0);
        assert_eq!(initial.matched_item_count, 0);
        assert!(!initial.items_changed);
        assert!(initial.matching);
        assert!(!initial.injecting);
        assert!(!initial.is_complete());
        assert_eq!(session.rank(), None);
        session.prepare_frame(frame_options()).complete();
        assert_eq!(session.match_status(), initial);

        update(&mut session);
        assert_eq!(session.match_status().item_count, 2);
        assert_eq!(session.match_status().matched_item_count, matched);
        let exit = Exit::Selected(session.events().hovered_id());
        assert_eq!(session.finish(exit).unwrap(), "alpha");
        assert_eq!(probe.calls[0], Call::Init(None, true));
    }
}

#[test]
fn construction_preserves_the_published_snapshot_after_query_edits_and_restarts() {
    let mut driver = driver(["alpha", "beta"]);
    driver.update_query("^beta$");
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    assert_eq!(session.query(), "^beta$");
    assert_eq!(session.match_status().matched_item_count, 2);
    assert!(session.match_status().matching);
    assert!(!session.match_status().items_changed);
    let exit = Exit::Selected(session.events().hovered_id());
    assert_eq!(session.finish(exit).unwrap(), "alpha");
    assert_eq!(probe.calls[0], Call::Init(Some("alpha".to_owned()), true));

    for reset_renderer in [false, true] {
        if reset_renderer {
            driver.reset_renderer(StrRenderer);
        } else {
            driver.restart();
        }
        driver.push_batch(["replacement".to_owned()]);
        let mut session = driver.session(Select);
        assert_eq!(session.match_status().item_count, 0);
        assert!(session.match_status().matching);
        assert!(!session.match_status().injecting);
        assert_eq!(session.rank(), None);
        update(&mut session);
        assert_eq!(session.match_status().matched_item_count, 1);
    }
}

#[test]
fn headless_updates_reconcile_the_hover_before_interface_initialization() {
    let mut driver = driver(["alpha", "beta", "gamma"]);
    driver
        .session(Select)
        .events()
        .match_list(MatchListEvent::Up(2));
    driver.update_query("^beta$");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !driver
        .update_matches(Duration::from_millis(5))
        .is_complete()
    {
        assert!(Instant::now() < deadline, "matching did not complete");
        std::thread::yield_now();
    }
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    assert_eq!(session.rank(), Some(0));
    assert_eq!(session.match_status().matched_item_count, 1);
    assert!(!session.match_status().items_changed);
    let exit = Exit::Selected(session.events().hovered_id());
    assert_eq!(session.finish(exit).unwrap(), "beta");
    assert_eq!(probe.calls[0], Call::Init(Some("beta".to_owned()), true));
}

#[test]
fn observations_do_not_advance_matching_or_run_frame_hooks() {
    let mut driver = driver(["alpha", "beta"]);
    let expected_hovered = driver.matches().get(1).unwrap().id();
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let initial = session.match_status();
    assert!(initial.is_complete());
    assert_eq!(initial.matched_item_count, 2);
    assert_eq!(session.query(), "");
    assert_eq!(session.rank(), Some(0));
    assert_eq!(session.queue_status(), None);
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::Up(1));
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
        assert_eq!(batch.hovered_id(), Some(expected_hovered));
        assert_eq!(batch.queue_status(), None);
    }
    for _ in 0..2 {
        assert_eq!(session.query(), "absent");
        assert_eq!(session.rank(), Some(1));
        assert_eq!(session.match_status(), initial);
        assert_eq!(session.queue_status(), None);
    }
    update(&mut session);
    assert_eq!(session.query(), "absent");
    assert_eq!(session.rank(), None);
    assert_eq!(session.match_status().matched_item_count, 0);
    assert!(session.match_status().is_complete());
    drop(session);
    assert_eq!(
        probe.calls,
        [Call::Init(Some("alpha".to_owned()), true), Call::End]
    );
}

#[test]
fn observations_preserve_pending_and_idle_frame_state() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![Components::PREVIEW].into_iter(),
        ..Probe::default()
    };
    let mut session = driver.session(&mut probe);
    session.prepare_frame(frame_options()).complete();
    for redraw in [Components::PREVIEW, Components::NONE] {
        assert_eq!(session.query(), "");
        assert_eq!(session.rank(), Some(0));
        assert_eq!(session.queue_status(), None);
        let status = session.match_status();
        let mut frame = session.prepare_frame(frame_options());
        assert_eq!(frame.redraw(), redraw);
        for _ in 0..2 {
            assert_eq!(frame.query(), "");
            assert_eq!(frame.rank(), Some(0));
            assert_eq!(frame.match_status(), status);
            assert_eq!(frame.queue_status(), None);
            assert_eq!(frame.prompt().query(), "");
            assert_eq!(frame.prompt().cursor(), None);
            let list = frame.match_list();
            assert_eq!(list.rank(), Some(0));
            assert!(
                list.matches()
                    .iter()
                    .map(|item| item.value().as_str())
                    .eq(["alpha", "beta"])
            );
            assert_eq!(frame.status().match_status(), status);
            assert_eq!(frame.status().queue_status(), None);
            assert!(matches!(frame.preview().content(), Content::Empty));
            assert_eq!(frame.redraw(), redraw);
            assert_eq!(frame.changed(), !redraw.is_none());
        }
        frame.complete();
    }
    drop(session);
    assert_eq!(probe.hook_counts(), (3, 3));
}

#[test]
fn batch_query_reads_allow_more_events_without_advancing_matching_or_frame_hooks() {
    let mut driver = driver(["alpha", "beta", "gamma"]);
    let expected_hovered = driver.matches().get(2).unwrap().id();
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let initial = session.match_status();
    session.prepare_frame(frame_options()).complete();
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::Up(1));
        batch.prompt(PromptEvent::Reset("gamm".to_owned()));
        assert_eq!(batch.query(), "gamm");
        batch.prompt(PromptEvent::Insert('a'));
        assert_eq!(batch.query(), "gamma");
        assert_eq!(batch.query(), "gamma");
        batch.match_list(MatchListEvent::Up(1));
        assert_eq!(batch.hovered_id(), Some(expected_hovered));
    }
    assert_eq!(session.query(), "gamma");
    assert_eq!(session.rank(), Some(2));
    assert_eq!(session.hovered().unwrap().id(), expected_hovered);
    assert_eq!(session.matches().len(), 3);
    assert_eq!(session.match_status(), initial);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().prompt);
    assert!(frame.redraw().list);
    frame.complete();

    {
        let mut batch = session.events();
        assert_eq!(batch.query(), "gamma");
        assert_eq!(batch.query(), "gamma");
    }
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
    update(&mut session);
    assert_eq!(session.matches().len(), 1);
    assert_eq!(session.hovered().unwrap().id(), expected_hovered);
    drop(session);
    assert_eq!(probe.hook_counts(), (3, 3));
}

#[test]
fn flushed_prompt_changes_survive_later_batches_and_matching_updates() {
    let mut driver = driver(["alpha", "beta"]);
    let mut session = driver.session(Select);
    session.prepare_frame(frame_options()).complete();
    {
        let mut batch = session.events();
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
        assert!(!batch.query().is_empty());
    }
    session.events().prompt(PromptEvent::Left(1));
    update(&mut session);
    session.update_matches(Duration::ZERO);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().prompt);
    assert!(frame.redraw().list);
    assert_eq!(frame.match_status().matched_item_count, 0);
    frame.complete();
    assert!(session.events().hovered_id().is_none());
    {
        let mut batch = session.events();
        batch.prompt(PromptEvent::Reset(String::new()));
        assert!(batch.query().is_empty());
    }
    drop(session);
    assert_eq!(driver.query(), "");
}

#[test]
fn preview_events_use_event_time_context_and_last_delivered_size_even_when_hidden() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let mut initial = session.prepare_frame(frame_options());
    assert!(initial.redraw().is_all());
    initial.preview().resize(Dimensions::default());
    initial.complete();
    {
        let mut batch = session.events();
        batch.set_visible_components(Components {
            preview: false,
            ..Components::ALL
        });
        batch.preview(PreviewEvent::Refresh);
    }
    let mut hidden = session.prepare_frame(frame_options());
    hidden.preview().resize(Dimensions::default());
    hidden.complete();
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::Up(1));
        batch.preview(PreviewEvent::Refresh);
    }
    let hidden = session.prepare_frame(frame_options());
    assert!(hidden.changed());
    assert!(!hidden.redraw().preview);
    hidden.complete();
    {
        let mut batch = session.events();
        batch.set_visible_components(Components::ALL);
        batch.preview(PreviewEvent::Refresh);
        batch.set_visible_components(Components {
            preview: false,
            ..Components::ALL
        });
        batch.set_visible_components(Components::ALL);
    }
    let mut frame = session.prepare_frame(frame_options());
    let size = Dimensions {
        width: 13,
        height: 7,
    };
    frame.preview().resize(size);
    frame.preview().resize(size);
    frame.complete();
    drop(session);
    assert_eq!(
        probe.calls,
        [
            Call::Init(Some("alpha".to_owned()), true),
            Call::Before(true, true),
            Call::Resize(Some(Dimensions::default())),
            Call::After,
            Call::Preview(Some("alpha".to_owned()), Some(Dimensions::default()), false),
            Call::Before(false, false),
            Call::Resize(None),
            Call::After,
            Call::Preview(Some("beta".to_owned()), None, false),
            Call::Before(false, false),
            Call::After,
            Call::Preview(Some("beta".to_owned()), None, true),
            Call::Before(false, true),
            Call::Resize(Some(Dimensions {
                width: 13,
                height: 7
            })),
            Call::After,
            Call::End,
        ]
    );
}

#[test]
fn enabled_preview_receives_events_without_items_or_drawable_space() {
    let mut driver = driver([]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    session.events().preview(PreviewEvent::Refresh);
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.rank(), None);
    frame.preview().resize(Dimensions::default());
    frame.complete();
    drop(session);
    assert!(probe.calls.contains(&Call::Preview(None, None, true)));
    assert!(
        probe
            .calls
            .contains(&Call::Resize(Some(Dimensions::default())))
    );
}

#[test]
fn idle_completion_retains_invalidations_and_abandoned_frames_require_a_full_repaint() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![Components::NONE, Components::PREVIEW].into_iter(),
        ..Probe::default()
    };
    let mut session = driver.session(&mut probe);
    let mut initial = session.prepare_frame(frame_options());
    initial.prompt().resize(Dimensions::default());
    initial.match_list().resize(Dimensions::default());
    initial.status().resize(Dimensions::default());
    initial.preview().resize(Dimensions::default());
    initial.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
    let abandoned = session.prepare_frame(frame_options());
    assert_eq!(abandoned.redraw(), Components::PREVIEW);
    drop(abandoned);
    let recovery = session.prepare_frame(frame_options());
    assert!(recovery.redraw().is_all());
    recovery.complete();

    let mut output = Vec::new();
    let mut idle = session.prepare_frame(frame_options());
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(Area::default(), ClearState::WithinRect);
    idle.prompt().draw(&mut rect).unwrap();
    idle.match_list().draw(&mut rect).unwrap();
    idle.status().draw(&mut rect).unwrap();
    idle.preview().draw(&mut rect).unwrap();
    assert!(!idle.changed());
    idle.complete();
    session.events().preview(PreviewEvent::Refresh);
    let mut forced = session.prepare_frame(frame_options());
    forced.prompt().draw(&mut rect).unwrap();
    forced.match_list().draw(&mut rect).unwrap();
    forced.status().draw(&mut rect).unwrap();
    forced.preview().draw(&mut rect).unwrap();
    assert!(output.is_empty());
    assert!(forced.changed());
    forced.complete();
    drop(session);
    assert_eq!(probe.hook_counts(), (6, 5));
}

#[test]
fn visibility_filters_redraw_without_repeating_preview_resize() {
    let status = Components {
        status: true,
        ..Components::NONE
    };
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![status, status].into_iter(),
        ..Probe::default()
    };
    let mut session = driver.session(&mut probe);
    session.events().set_visible_components(Components::PREVIEW);
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions::default());
    initial.complete();

    let hidden = session.prepare_frame(frame_options());
    assert!(hidden.changed());
    assert!(hidden.redraw().is_none());
    hidden.complete();
    session
        .events()
        .set_visible_components(status | Components::PREVIEW);
    let visible = session.prepare_frame(frame_options());
    assert!(visible.changed());
    assert_eq!(visible.redraw(), status);
    visible.complete();

    session.events().preview(PreviewEvent::Refresh);
    let mut frame = session.prepare_frame(frame_options());
    assert!(frame.changed());
    assert_eq!(frame.redraw(), Components::PREVIEW);
    frame.preview().resize(Dimensions::default());
    assert!(frame.changed());
    frame.complete();
    {
        let mut batch = session.events();
        batch.set_visible_components(Components::NONE);
        batch.set_visible_components(status | Components::PREVIEW);
    }
    let mut frame = session.prepare_frame(frame_options());
    assert!(frame.changed());
    frame.preview().resize(Dimensions::default());
    frame.complete();
    drop(session);
    assert_eq!(
        probe
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Resize(_)))
            .count(),
        1
    );
}

#[test]
fn hidden_component_invalidations_are_acknowledged_by_completion() {
    for pending in [Components::PREVIEW, Components::ALL] {
        let mut driver = driver(["alpha"]);
        let mut probe = Probe {
            after_redraw: vec![pending].into_iter(),
            ..Probe::default()
        };
        let mut session = driver.session(&mut probe);
        session.events().set_visible_components(Components::NONE);
        session.prepare_frame(frame_options()).complete();

        let hidden = session.prepare_frame(frame_options());
        assert!(hidden.changed());
        assert!(hidden.redraw().is_none());
        hidden.complete();

        let idle = session.prepare_frame(frame_options());
        assert!(!idle.changed());
        assert!(idle.redraw().is_none());
        idle.complete();
    }
}

#[test]
fn visibility_changes_persist_and_request_redraw_until_completed() {
    let mut driver = driver(["alpha"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    assert!(session.visible_components().is_all());
    session.prepare_frame(frame_options()).complete();

    for (visible, redraw) in [
        (
            Components {
                status: false,
                ..Components::ALL
            },
            Components::NONE,
        ),
        (
            Components {
                prompt: false,
                ..Components::ALL
            },
            Components {
                status: true,
                ..Components::NONE
            },
        ),
        (
            Components {
                list: false,
                ..Components::ALL
            },
            Components {
                prompt: true,
                ..Components::NONE
            },
        ),
        (
            Components {
                preview: false,
                ..Components::ALL
            },
            Components {
                list: true,
                ..Components::NONE
            },
        ),
        (Components::NONE, Components::NONE),
        (Components::ALL, Components::ALL),
    ] {
        {
            let mut batch = session.events();
            batch.set_visible_components(visible);
            assert_eq!(batch.visible_components(), visible);
        }
        session.update_matches(Duration::ZERO);
        assert_eq!(session.visible_components(), visible);
        let frame = session.prepare_frame(frame_options());
        assert!(frame.changed());
        assert_eq!(frame.visible_components(), visible);
        assert_eq!(frame.redraw(), redraw);
        drop(frame);

        let retry = session.prepare_frame(frame_options());
        assert!(retry.changed());
        assert_eq!(retry.visible_components(), visible);
        assert_eq!(retry.redraw(), visible);
        retry.complete();

        session.events().set_visible_components(visible);
        let idle = session.prepare_frame(frame_options());
        assert_eq!(idle.visible_components(), visible);
        assert!(!idle.changed());
        idle.complete();
    }
}

#[test]
fn unsupported_preview_is_excluded_without_requesting_a_redraw() {
    let mut driver = driver(["alpha"]);
    let mut session = driver.session(Select);
    let supported = Components {
        preview: false,
        ..Components::ALL
    };
    assert_eq!(session.visible_components(), supported);
    session.prepare_frame(frame_options()).complete();
    {
        let mut batch = session.events();
        batch.set_visible_components(Components::ALL);
        assert_eq!(batch.visible_components(), supported);
        batch.preview(PreviewEvent::Refresh);
    }
    let mut frame = session.prepare_frame(frame_options());
    assert!(!frame.changed());
    assert_eq!(frame.visible_components(), supported);
    assert!(matches!(frame.preview().content(), Content::Empty));
    let mut surface = TestSurface::new(12, 5);
    frame
        .preview()
        .border(BoundaryChars::new())
        .draw(&mut surface)
        .unwrap();
    assert!(!surface.touched);
    assert!(!frame.changed());
    frame.complete();

    session.events().set_visible_components(Components::PREVIEW);
    let hidden = session.prepare_frame(frame_options());
    assert!(hidden.changed());
    assert!(hidden.visible_components().is_none());
    hidden.complete();
    session.events().set_visible_components(Components::PREVIEW);
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
}

#[test]
fn hidden_components_do_not_paint_nonempty_targets_or_expose_a_cursor() {
    let mut driver = driver(["alpha"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    for visible in [Components::NONE, Components::ALL] {
        session.events().set_visible_components(visible);
        let mut frame = session.prepare_frame(frame_options());
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        let mut rect = target.rect(
            Area {
                column: 0,
                row: 0,
                width: 20,
                height: 5,
            },
            ClearState::WithinRect,
        );
        frame.prompt().draw(&mut rect).unwrap();
        frame.match_list().draw(&mut rect).unwrap();
        frame.status().draw(&mut rect).unwrap();
        frame.preview().draw(&mut rect).unwrap();
        assert_eq!(output.is_empty(), visible.is_none());
        assert_eq!(frame.prompt().cursor().is_some(), visible.prompt);
        frame.complete();
    }
}

#[test]
fn content_invalidations_coalesce_across_batches_until_frame_completion() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    session.prepare_frame(frame_options()).complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();

    {
        let mut batch = session.events();
        batch.preview(PreviewEvent::Refresh);
        batch.preview(PreviewEvent::Refresh);
    }
    session.events().preview(PreviewEvent::Refresh);
    drop(session.events());
    session.update_matches(Duration::ZERO);

    let abandoned = session.prepare_frame(frame_options());
    assert!(abandoned.changed());
    assert_eq!(abandoned.redraw(), Components::PREVIEW);
    assert_eq!(abandoned.query(), "");
    assert_eq!(abandoned.rank(), Some(0));
    drop(abandoned);

    let retry = session.prepare_frame(frame_options());
    assert!(retry.changed());
    assert!(retry.redraw().is_all());
    retry.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    assert!(idle.redraw().is_none());
    idle.complete();
    drop(session);
    assert_eq!(probe.hook_counts(), (5, 4));
}

#[test]
fn a_failed_paint_can_be_retried_without_repeating_frame_hooks_or_preview_resize() {
    struct FailOnce(bool);

    impl io::Write for FailOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if std::mem::take(&mut self.0) {
                Err(io::Error::other("paint failed"))
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let mut frame = session.prepare_frame(frame_options());
    let area = Area {
        column: 9,
        row: 4,
        width: 13,
        height: 7,
    };
    let mut writer = FailOnce(true);
    assert!(
        frame
            .preview()
            .draw(&mut CrosstermTarget::new(&mut writer).rect(area, ClearState::WithinRect))
            .is_err()
    );
    frame
        .preview()
        .draw(&mut CrosstermTarget::new(&mut writer).rect(area, ClearState::WithinRect))
        .unwrap();
    frame.complete();
    drop(session);
    assert_eq!(probe.hook_counts(), (1, 1));
    assert_eq!(
        probe
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Resize(_)))
            .collect::<Vec<_>>(),
        [&Call::Resize(Some(Dimensions {
            width: 13,
            height: 7,
        }))]
    );
}

#[test]
fn preview_resize_delivers_first_changed_zero_and_restored_content_sizes() {
    for draw in [false, true] {
        let mut driver = driver(["alpha"]);
        let mut probe = Probe::default();
        let mut session = driver.session(&mut probe);
        for (width, height) in [
            (13, 7),
            (13, 7),
            (0, 0),
            (0, 0),
            (13, 7),
            (1, 2),
            (2, 1),
            (0, 7),
            (13, 0),
            (13, 7),
        ] {
            let size = Dimensions { width, height };
            let mut frame = session.prepare_frame(frame_options());
            if draw {
                let mut output = Vec::new();
                frame
                    .preview()
                    .draw(&mut CrosstermTarget::new(&mut output).rect(
                        Area {
                            column: 9,
                            row: 4,
                            width,
                            height,
                        },
                        ClearState::WithinRect,
                    ))
                    .unwrap();
                if width == 0 || height == 0 {
                    assert!(output.is_empty());
                }
            } else {
                frame.preview().resize(size);
            }
            frame.complete();
        }
        drop(session);
        assert_eq!(
            probe
                .calls
                .iter()
                .filter_map(|call| match call {
                    Call::Resize(size) => Some(*size),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [
                (13, 7),
                (0, 0),
                (13, 7),
                (1, 2),
                (2, 1),
                (0, 7),
                (13, 0),
                (13, 7)
            ]
            .map(|(width, height)| Some(Dimensions { width, height })),
            "draw={draw}",
        );
    }
}

#[test]
fn prepared_preview_delivers_content_sizes_for_bordered_targets() {
    for draw in [false, true] {
        let mut driver = driver(["alpha"]);
        let mut probe = Probe::default();
        let mut session = driver.session(&mut probe);
        for (width, height, visible) in [
            (13, 7, true),
            (13, 7, true),
            (1, 2, true),
            (2, 1, true),
            (0, 7, true),
            (13, 0, true),
            (13, 7, false),
            (13, 7, false),
            (13, 7, true),
        ] {
            session.events().set_visible_components(Components {
                preview: visible,
                ..Components::ALL
            });
            let mut frame = session.prepare_frame(frame_options());
            let mut preview = frame.preview().border(BoundaryChars::new());
            preview.resize(Dimensions { width, height });
            if draw {
                let mut surface = TestSurface::new(width, height);
                preview.draw(&mut surface).unwrap();
                assert_eq!(surface.touched, visible && width != 0 && height != 0);
            }
            frame.complete();
        }
        drop(session);
        assert_eq!(probe.hook_counts(), (9, 9));
        assert_eq!(
            probe
                .calls
                .iter()
                .filter_map(|call| match call {
                    Call::Resize(size) => Some(*size),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [
                Some(Dimensions {
                    width: 11,
                    height: 5
                }),
                Some(Dimensions::default()),
                Some(Dimensions {
                    width: 0,
                    height: 5
                }),
                Some(Dimensions {
                    width: 11,
                    height: 0
                }),
                None,
                Some(Dimensions {
                    width: 11,
                    height: 5
                }),
            ],
            "draw={draw}",
        );
    }
}

#[test]
fn prepared_preview_border_changes_do_not_invalidate_or_repeat_content_resize() {
    let mut driver = driver(["alpha"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions {
        width: 13,
        height: 7,
    });
    initial.complete();
    for (width, height, border) in [
        (13, 7, None),
        (15, 9, Some(BoundaryChars::new())),
        (15, 9, Some(BoundaryChars::new())),
        (15, 9, Some(BoundaryChars::ascii())),
        (15, 9, Some(BoundaryChars::ascii())),
        (13, 7, None),
        (13, 7, None),
    ] {
        let mut frame = session.prepare_frame(frame_options());
        assert!(!frame.changed());
        {
            let _preview = frame.preview().border(BoundaryChars::ascii());
        }
        assert!(!frame.changed());
        let mut preview = frame.preview();
        if let Some(chars) = border {
            preview = preview.border(chars);
        }
        preview.resize(Dimensions { width, height });
        assert!(!frame.changed());
        frame.complete();
    }
    drop(session);
    assert_eq!(probe.hook_counts(), (8, 8));
    assert_eq!(
        probe
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Resize(_)))
            .collect::<Vec<_>>(),
        [&Call::Resize(Some(Dimensions {
            width: 13,
            height: 7
        }))],
    );
}

#[test]
fn initially_disabled_preview_is_distinct_from_enabled_zero_size() {
    let mut driver = driver(["alpha"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    for enabled in [false, false, true, true, false, true] {
        session.events().set_visible_components(Components {
            preview: enabled,
            ..Components::ALL
        });
        let mut frame = session.prepare_frame(frame_options());
        let mut output = Vec::new();
        frame
            .preview()
            .draw(
                &mut CrosstermTarget::new(&mut output)
                    .rect(Area::default(), ClearState::WithinRect),
            )
            .unwrap();
        assert!(output.is_empty());
        frame.complete();
    }
    drop(session);
    assert_eq!(
        probe
            .calls
            .iter()
            .filter_map(|call| match call {
                Call::Resize(size) => Some(*size),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            None,
            Some(Dimensions::default()),
            None,
            Some(Dimensions::default()),
        ],
    );
}

#[test]
fn repainting_unchanged_widgets_into_fresh_buffers_preserves_logical_frame_state() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe);
    let mut previous = None;
    for changed in [true, false] {
        let mut frame = session.prepare_frame(frame_options());
        assert_eq!(frame.changed(), changed);
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        frame
            .prompt()
            .draw(&mut target.rect(
                Area {
                    column: 7,
                    row: 11,
                    width: 20,
                    height: 1,
                },
                ClearState::WithinRect,
            ))
            .unwrap();
        frame
            .match_list()
            .draw(&mut target.rect(
                Area {
                    column: 7,
                    row: 3,
                    width: 20,
                    height: 7,
                },
                ClearState::WithinRect,
            ))
            .unwrap();
        frame
            .status()
            .draw(&mut target.rect(
                Area {
                    column: 7,
                    row: 10,
                    width: 20,
                    height: 1,
                },
                ClearState::WithinRect,
            ))
            .unwrap();
        frame
            .preview()
            .draw(&mut target.rect(
                Area {
                    column: 28,
                    row: 3,
                    width: 13,
                    height: 9,
                },
                ClearState::WithinRect,
            ))
            .unwrap();
        assert_eq!(frame.changed(), changed);
        assert_eq!(
            frame.prompt().cursor(),
            Some(Position { column: 2, row: 0 })
        );
        assert!(String::from_utf8_lossy(&output).contains("alpha"));
        if let Some(previous) = &previous {
            assert_eq!(previous, &output);
        }
        previous = Some(output);
        frame.complete();
    }
    drop(session);
    assert_eq!(probe.hook_counts(), (2, 2));
    assert_eq!(
        probe
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Resize(_)))
            .count(),
        1,
    );
}

#[test]
fn prompt_cursor_is_local_and_disappears_for_empty_targets() {
    let mut driver = driver(["alpha"]);
    let mut session = driver.session(Select);
    session
        .events()
        .prompt(PromptEvent::Reset("abc".to_owned()));
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.prompt().cursor(), None);
    let size = Dimensions {
        width: 20,
        height: 1,
    };
    frame.prompt().resize(size);
    assert_eq!(
        frame.prompt().cursor(),
        Some(Position { column: 5, row: 0 })
    );
    for (width, height) in [(0, 1), (20, 0), (20, 1)] {
        let mut output = Vec::new();
        frame
            .prompt()
            .draw(&mut CrosstermTarget::new(&mut output).rect(
                Area {
                    column: 9,
                    row: 4,
                    width,
                    height,
                },
                ClearState::WithinRect,
            ))
            .unwrap();
        if width == 0 || height == 0 {
            assert_eq!(frame.prompt().cursor(), None);
            assert!(output.is_empty());
        } else {
            assert_eq!(
                frame.prompt().cursor(),
                Some(Position { column: 5, row: 0 })
            );
            assert!(!output.is_empty());
        }
    }
    frame.complete();
}

#[test]
fn size_only_prompt_preparation_sets_local_cursor_and_subsequent_paint() {
    let mut driver = driver(["alpha"]);
    let mut session = driver.session(Select);
    session
        .events()
        .prompt(PromptEvent::Reset("ＡＡＡＡＡ".to_owned()));
    for (width, painted, column) in [
        (9, ">  ＡＡ", Some(7)),
        (0, "", None),
        (20, "> ＡＡＡＡＡ", Some(12)),
    ] {
        let mut frame = session.prepare_frame(frame_options());
        frame.prompt().resize(Dimensions { width, height: 1 });
        assert_eq!(frame.prompt().query(), "ＡＡＡＡＡ");
        assert_eq!(
            frame.prompt().cursor(),
            column.map(|column| Position { column, row: 0 })
        );
        let mut target = TestSurface::new(width, 1);
        frame.prompt().draw(&mut target).unwrap();
        assert_eq!(target.rows, [painted]);
        assert_eq!(
            frame.prompt().cursor(),
            column.map(|column| Position { column, row: 0 })
        );
        frame.complete();
    }
}

#[test]
fn grapheme_width_is_used_for_prompt_events_between_frames() {
    let mut driver = driver(["alpha"]);
    let mut session = driver.session(Select);
    let mut frame = session.prepare_frame(frame_options());
    frame.prompt().resize(Dimensions {
        width: 12,
        height: 1,
    });
    frame.complete();
    {
        let mut batch = session.events();
        batch.prompt(PromptEvent::Reset("ＡＡ".to_owned()));
        batch.prompt(PromptEvent::Left(1));
        batch.prompt(PromptEvent::Insert('x'));
    }
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.query(), "ＡxＡ");
    assert_eq!(
        frame.prompt().cursor(),
        Some(Position { column: 5, row: 0 })
    );
    let mut target = TestSurface::new(12, 1);
    frame.prompt().draw(&mut target).unwrap();
    assert_eq!(target.rows, ["> ＡxＡ"]);
    frame.complete();
}

#[cfg(feature = "ratatui")]
#[test]
fn ratatui_prompt_edits_use_grapheme_widths_at_unchanged_dimensions() {
    use ratatui_core::{buffer::Buffer, layout::Rect};

    use crate::draw::ratatui::RatatuiSurface;

    for draw_first in [false, true] {
        let mut driver = driver(["alpha"]);
        let mut session = driver.session(Select);
        let area = Rect::new(9, 4, 12, 1);
        let mut buffer = Buffer::empty(area);
        let mut frame = session.prepare_frame(frame_options());
        if draw_first {
            frame
                .prompt()
                .draw(&mut RatatuiSurface::new(&mut buffer, area))
                .unwrap();
        } else {
            frame.prompt().resize(Dimensions {
                width: area.width,
                height: area.height,
            });
        }
        frame.complete();
        {
            let mut batch = session.events();
            batch.prompt(PromptEvent::Paste("ｶﾞ".to_owned()));
            batch.prompt(PromptEvent::Insert('x'));
            batch.prompt(PromptEvent::Left(1));
        }
        let mut frame = session.prepare_frame(frame_options());
        assert_eq!(frame.query(), "ｶﾞx");
        assert_eq!(
            frame.prompt().cursor(),
            Some(Position { column: 4, row: 0 })
        );
        frame
            .prompt()
            .draw(&mut RatatuiSurface::new(&mut buffer, area))
            .unwrap();
        assert_eq!(
            frame.prompt().cursor(),
            Some(Position { column: 4, row: 0 })
        );
        assert_eq!(buffer[(area.x + 2, area.y)].symbol(), "ｶﾞ");
        assert_eq!(buffer[(area.x + 4, area.y)].symbol(), "x");
        frame.complete();
    }
}

#[test]
fn abandonment_commits_edits_and_unwinding_skips_interface_cleanup() {
    for panic in [false, true] {
        let mut driver = driver(["alpha", "beta"]);
        let mut probe = Probe::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut session = driver.session(&mut probe);
            let mut batch = session.events();
            batch.prompt(PromptEvent::Insert('x'));
            batch.match_list(MatchListEvent::Up(1));
            assert!(!panic, "unwind with an active batch");
        }));
        assert_eq!(result.is_err(), panic);
        assert_eq!(driver.query(), "x");
        assert_eq!(
            probe
                .calls
                .iter()
                .filter(|call| **call == Call::End)
                .count(),
            usize::from(!panic)
        );
        assert!(!probe.calls.contains(&Call::Finish));
        assert!(driver.filter().unwrap().is_empty());
    }
}
