use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use super::{Select, TestTerminal, driver, frame_options, update};
use crate::{
    Terminal,
    event::{MatchListEvent, PromptEvent},
    preview::PreviewEvent,
    rect::Area,
    session::{Context, Dimensions, Exit, Geometry, Interface, Redraw},
};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Init(Option<String>, bool),
    Before(bool, bool),
    After,
    Resize(Option<Dimensions>),
    Preview(Option<String>, Option<Dimensions>),
    End,
    Finish,
}

#[derive(Default)]
struct Probe {
    calls: Vec<Call>,
    after_redraw: std::vec::IntoIter<Redraw>,
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
    ) -> Redraw {
        self.calls.push(Call::Before(changed, cx.preview_enabled()));
        Redraw::NONE
    }

    fn after_frame(&mut self, _: &mut (), _: &Context<'_, String>, _: Instant) -> Redraw {
        self.calls.push(Call::After);
        self.after_redraw.next().unwrap_or(Redraw::NONE)
    }

    fn preview_resize(&mut self, _: &mut (), size: Option<Dimensions>) {
        self.calls.push(Call::Resize(size));
        self.size = size;
    }

    fn preview_event(&mut self, _: &mut (), _: PreviewEvent, cx: &Context<'_, String>) -> Redraw {
        assert!(cx.preview_enabled());
        self.calls.push(Call::Preview(
            cx.hovered().map(|item| item.value().clone()),
            self.size,
        ));
        Redraw::PREVIEW
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
fn flushed_prompt_changes_survive_later_batches_and_matching_updates() {
    let mut driver = driver(["alpha", "beta"]);
    let mut session = driver.session(Select, Duration::ZERO);
    let mut initial = session.prepare_frame(frame_options());
    initial.resize(Geometry::default());
    initial.complete();
    {
        let mut batch = session.events();
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
        assert!(!batch.prompt_is_empty());
    }
    session.events().prompt(PromptEvent::Left(1));
    update(&mut session);
    session.update(Duration::ZERO);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().prompt);
    assert!(frame.redraw().list);
    assert!(frame.view().context().matches().is_empty());
    frame.complete();
    assert!(session.events().selection_exit().is_none());
    {
        let mut batch = session.events();
        batch.prompt(PromptEvent::Reset(String::new()));
        assert!(batch.prompt_is_empty());
    }
    drop(session);
    assert_eq!(driver.query(), "");
}

#[test]
fn preview_events_use_immediate_enablement_and_navigation_but_last_delivered_geometry() {
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe, Duration::ZERO);
    let mut initial = session.prepare_frame(frame_options());
    assert_eq!(initial.redraw(), Redraw::FULL);
    assert!(initial.view().geometry().is_none());
    initial.resize(Geometry::default());
    initial.complete();
    {
        let mut batch = session.events();
        batch.set_preview_enabled(false);
        batch.preview(PreviewEvent::Refresh);
    }
    let mut hidden = session.prepare_frame(frame_options());
    hidden.resize(Geometry::default());
    hidden.complete();
    {
        let mut batch = session.events();
        batch.set_preview_enabled(true);
        batch.match_list(MatchListEvent::Up(1));
        batch.preview(PreviewEvent::Refresh);
        batch.set_preview_enabled(false);
        batch.set_preview_enabled(true);
    }
    let mut frame = session.prepare_frame(frame_options());
    let geometry = Geometry {
        dimensions: Dimensions {
            width: 40,
            height: 20,
        },
        preview: Area {
            column: 9,
            row: 4,
            width: 13,
            height: 7,
        },
        ..Geometry::default()
    };
    frame.resize(geometry);
    frame.resize(geometry);
    assert_eq!(frame.view().geometry(), Some(geometry));
    frame.complete();
    drop(session);
    assert_eq!(
        probe.calls,
        [
            Call::Init(Some("alpha".to_owned()), true),
            Call::Before(true, true),
            Call::Resize(Some(Dimensions::default())),
            Call::After,
            Call::Before(false, false),
            Call::Resize(None),
            Call::After,
            Call::Preview(Some("beta".to_owned()), None),
            Call::Before(false, true),
            Call::Resize(Some(Dimensions {
                width: 11,
                height: 5
            })),
            Call::After,
            Call::End,
        ]
    );
    let mut session = driver.session(Select, Duration::ZERO);
    let mut batch = session.events();
    batch.toggle_preview_enabled();
    assert!(!batch.preview_enabled());
}

#[test]
fn enabled_preview_receives_events_without_items_or_drawable_space() {
    let mut driver = driver([]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe, Duration::ZERO);
    session.events().preview(PreviewEvent::Refresh);
    let mut frame = session.prepare_frame(frame_options());
    assert!(frame.view().context().hovered().is_none());
    frame.resize(Geometry::default());
    frame.complete();
    drop(session);
    assert!(probe.calls.contains(&Call::Preview(None, None)));
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
        after_redraw: vec![Redraw::NONE, Redraw::PREVIEW].into_iter(),
        ..Probe::default()
    };
    let mut session = driver.session(&mut probe, Duration::ZERO);
    let mut initial = session.prepare_frame(frame_options());
    initial.resize(Geometry::default());
    initial.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.needs_render());
    idle.complete();
    let abandoned = session.prepare_frame(frame_options());
    assert_eq!(abandoned.redraw(), Redraw::PREVIEW);
    drop(abandoned);
    let recovery = session.prepare_frame(frame_options());
    assert_eq!(recovery.redraw(), Redraw::FULL);
    recovery.complete();

    let mut terminal = TestTerminal::default();
    let mut idle = session.prepare_frame(frame_options());
    idle.draw(&mut terminal, Redraw::FULL).unwrap();
    assert!(!idle.changed());
    idle.complete();
    let mut options = frame_options();
    options.force_redraw = true;
    let mut forced = session.prepare_frame(options);
    forced.draw(&mut terminal, forced.redraw()).unwrap();
    assert!(terminal.output.is_empty());
    assert!(forced.changed());
    forced.complete();
    drop(session);
    assert_eq!(probe.hook_counts(), (6, 5));
}

#[test]
fn visibility_filters_invalidations_and_equal_geometry_still_delivers_pending_layout_work() {
    let status = Redraw {
        status: true,
        ..Redraw::NONE
    };
    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![status, status].into_iter(),
        ..Probe::default()
    };
    let mut session = driver.session(&mut probe, Duration::ZERO);
    let mut initial = session.prepare_frame(frame_options());
    initial.resize(Geometry::default());
    initial.complete();

    let mut options = frame_options();
    options.visible_components = Redraw::NONE;
    let hidden = session.prepare_frame(options);
    assert!(!hidden.needs_render());
    assert!(!hidden.changed());
    assert_eq!(hidden.redraw(), Redraw::NONE);
    hidden.complete();
    let mut options = frame_options();
    options.visible_components = status;
    let visible = session.prepare_frame(options);
    assert!(visible.needs_render());
    assert_eq!(visible.redraw(), status);
    visible.complete();

    let mut options = frame_options();
    options.visible_components = Redraw::NONE;
    options.layout_changed = true;
    let mut frame = session.prepare_frame(options);
    assert!(frame.needs_render());
    assert_eq!(frame.redraw(), Redraw::NONE);
    frame.resize(Geometry::default());
    assert!(frame.changed());
    frame.complete();
    {
        let mut batch = session.events();
        batch.toggle_preview_enabled();
        batch.toggle_preview_enabled();
    }
    let mut frame = session.prepare_frame(frame_options());
    assert!(frame.needs_render());
    frame.resize(Geometry::default());
    frame.complete();
    drop(session);
    assert_eq!(
        probe
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Resize(_)))
            .count(),
        3
    );
}

#[test]
fn a_failed_paint_can_be_retried_without_repeating_frame_hooks() {
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

    impl Terminal for FailOnce {
        fn init(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn cleanup(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn dimensions(&mut self) -> io::Result<Dimensions> {
            unreachable!()
        }
    }

    let mut driver = driver(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = driver.session(&mut probe, Duration::ZERO);
    let mut frame = session.prepare_frame(frame_options());
    frame.resize(Geometry {
        dimensions: Dimensions {
            width: 20,
            height: 8,
        },
        prompt: Area {
            column: 0,
            row: 7,
            width: 20,
            height: 1,
        },
        ..Geometry::default()
    });
    let mut terminal = FailOnce(true);
    assert!(frame.draw(&mut terminal, Redraw::FULL).is_err());
    frame.draw(&mut terminal, Redraw::FULL).unwrap();
    frame.complete();
    drop(session);
    assert_eq!(probe.hook_counts(), (1, 1));
}

#[test]
fn abandonment_commits_edits_and_unwinding_skips_interface_cleanup() {
    for panic in [false, true] {
        let mut driver = driver(["alpha", "beta"]);
        let mut probe = Probe::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut session = driver.session(&mut probe, Duration::ZERO);
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
