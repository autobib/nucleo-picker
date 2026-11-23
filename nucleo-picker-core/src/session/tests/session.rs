use std::{
    fmt, io,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use super::{Select, TestSurface, frame_options, model, update};
use crate::{
    draw::{Style, StyledStr, Surface},
    event::{MatchListEvent, PreviewEvent, PromptEvent},
    interface::Interface,
    preview::BoundaryChars,
    render::StrRenderer,
    session::{Components, Context, Dimensions, Exit, ModelOptions, Position},
};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Init(Option<String>, bool),
    Before,
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
                .filter(|call| **call == Call::Before)
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
            cx.active_components().preview,
        ));
    }

    fn before_frame(
        &mut self,
        _: &mut (),
        _: &Context<'_, String>,
        _: bool,
        _: Instant,
    ) -> Components {
        self.calls.push(Call::Before);
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
            cx.active_components().preview,
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
    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
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
        frame
            .prompt()
            .draw(&mut TestSurface::new(width, height))
            .unwrap();
        frame
            .match_list()
            .draw(&mut TestSurface::new(width, height))
            .unwrap();
        frame
            .status()
            .draw(&mut TestSurface::new(width, height))
            .unwrap();
        frame
            .preview()
            .draw(&mut TestSurface::new(width, height))
            .unwrap();
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
        let mut model = ModelOptions::new()
            .sort_results(false)
            .query(query)
            .model(StrRenderer);
        model.push_batch(["alpha", "beta"].map(str::to_owned));
        let generation = model.generation();
        let mut session = model.session(&mut probe);
        assert_eq!(session.generation(), generation);
        assert_eq!(session.events().generation(), generation);
        let initial = session.match_status();
        assert_eq!(initial.item_count, 0);
        assert_eq!(initial.matched_item_count, 0);
        assert!(!initial.items_changed);
        assert!(initial.matching);
        assert!(!initial.injecting);
        assert_eq!(session.hovered_rank(), None);
        session.prepare_frame(frame_options()).complete();
        assert_eq!(session.match_status(), initial);

        update(&mut session);
        assert_eq!(session.match_status().item_count, 2);
        assert_eq!(session.match_status().matched_item_count, matched);
        drop(session);
        assert_eq!(probe.calls[0], Call::Init(None, true));
    }
}

#[test]
fn construction_preserves_the_published_snapshot_after_query_edits_and_restarts() {
    let mut model = model(["alpha", "beta"]);
    model.update_query("^beta$");
    let mut probe = Probe::default();
    let session = model.session(&mut probe);
    assert_eq!(session.query(), "^beta$");
    assert_eq!(session.match_status().matched_item_count, 2);
    assert!(session.match_status().matching);
    assert!(!session.match_status().items_changed);
    drop(session);
    assert_eq!(probe.calls[0], Call::Init(Some("alpha".to_owned()), true));

    for reset_renderer in [false, true] {
        if reset_renderer {
            model.reset_renderer(StrRenderer);
        } else {
            model.restart();
        }
        model.push_batch(["replacement".to_owned()]);
        let mut session = model.session(Select);
        assert_eq!(session.match_status().item_count, 0);
        assert!(session.match_status().matching);
        assert!(!session.match_status().injecting);
        assert_eq!(session.hovered_rank(), None);
        update(&mut session);
        assert_eq!(session.match_status().matched_item_count, 1);
    }
}

#[test]
fn headless_updates_reconcile_the_hover_before_interface_initialization() {
    let mut model = model(["alpha", "beta", "gamma"]);
    model
        .session(Select)
        .events()
        .match_list(MatchListEvent::Up(2));
    model.update_query("^beta$");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !model.update_matches(Duration::from_millis(5)).is_complete() {
        assert!(Instant::now() < deadline, "matching did not complete");
        std::thread::yield_now();
    }
    let mut probe = Probe::default();
    let session = model.session(&mut probe);
    assert_eq!(session.hovered_rank(), Some(0));
    assert_eq!(session.match_status().matched_item_count, 1);
    assert!(!session.match_status().items_changed);
    drop(session);
    assert_eq!(probe.calls[0], Call::Init(Some("beta".to_owned()), true));
}

#[test]
fn observations_do_not_advance_matching_or_run_frame_hooks() {
    let mut model = model(["alpha", "beta"]);
    let expected_hovered = model.matches().get(1).unwrap().id();
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    let initial = session.match_status();
    assert_eq!(initial.matched_item_count, 2);
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::Up(1));
        batch.prompt(PromptEvent::Reset("absent".to_owned()));
        assert_eq!(batch.hovered_id(), Some(expected_hovered));
    }
    assert_eq!(session.query(), "absent");
    assert_eq!(session.hovered_rank(), Some(1));
    assert_eq!(session.match_status(), initial);
    update(&mut session);
    assert_eq!(session.hovered_rank(), None);
    assert_eq!(session.match_status().matched_item_count, 0);
    drop(session);
    assert_eq!(
        probe.calls,
        [Call::Init(Some("alpha".to_owned()), true), Call::End]
    );
}

#[test]
fn observations_preserve_pending_and_idle_frame_state() {
    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![Components::PREVIEW].into_iter(),
        ..Probe::default()
    };
    let mut session = model.session(&mut probe);
    session.prepare_frame(frame_options()).complete();
    for redraw in [Components::PREVIEW, Components::NONE] {
        let hovered = session.hovered().unwrap().id();
        let status = session.match_status();
        let mut frame = session.prepare_frame(frame_options());
        assert_eq!(frame.redraw(), redraw);
        assert_eq!(frame.matches().len(), 2);
        assert_eq!(frame.hovered().unwrap().id(), hovered);
        assert_eq!(frame.status().match_status(), status);
        assert_eq!(frame.redraw(), redraw);
        assert_eq!(frame.changed(), !redraw.is_none());
        frame.complete();
    }
    drop(session);
    assert_eq!(probe.hook_counts(), (3, 3));
}

#[test]
fn batch_query_reads_allow_more_events_without_advancing_matching_or_frame_hooks() {
    let mut model = model(["alpha", "beta", "gamma"]);
    let expected_hovered = model.matches().get(2).unwrap().id();
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    let initial = session.match_status();
    session.prepare_frame(frame_options()).complete();
    {
        let mut batch = session.events();
        batch.match_list(MatchListEvent::Up(1));
        batch.prompt(PromptEvent::Reset("gamm".to_owned()));
        assert_eq!(batch.query(), "gamm");
        batch.prompt(PromptEvent::Insert('a'));
        assert_eq!(batch.query(), "gamma");
        batch.match_list(MatchListEvent::Up(1));
        assert_eq!(batch.hovered_id(), Some(expected_hovered));
    }
    assert_eq!(session.hovered().unwrap().id(), expected_hovered);
    assert_eq!(session.matches().len(), 3);
    assert_eq!(session.match_status(), initial);
    let frame = session.prepare_frame(frame_options());
    assert!(frame.redraw().prompt);
    assert!(frame.redraw().match_list);
    frame.complete();

    {
        let mut batch = session.events();
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
    let mut model = model(["alpha", "beta"]);
    let mut session = model.session(Select);
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
    assert!(frame.redraw().match_list);
    assert_eq!(frame.match_status().matched_item_count, 0);
    frame.complete();
}

#[test]
fn preview_events_use_event_time_context_and_last_delivered_size_even_when_hidden() {
    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions::default());
    initial.complete();
    {
        let mut batch = session.events();
        batch.set_active_components(Components {
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
        batch.set_active_components(Components::ALL);
        batch.preview(PreviewEvent::Refresh);
    }
    let mut frame = session.prepare_frame(frame_options());
    let size = Dimensions {
        width: 13,
        height: 7,
    };
    frame.preview().resize(size);
    frame.complete();
    drop(session);
    assert_eq!(
        probe
            .calls
            .into_iter()
            .filter(|call| matches!(call, Call::Preview(..) | Call::Resize(_)))
            .collect::<Vec<_>>(),
        [
            Call::Resize(Some(Dimensions::default())),
            Call::Preview(Some("alpha".to_owned()), Some(Dimensions::default()), false),
            Call::Resize(None),
            Call::Preview(Some("beta".to_owned()), None, false),
            Call::Preview(Some("beta".to_owned()), None, true),
            Call::Resize(Some(Dimensions {
                width: 13,
                height: 7
            })),
        ]
    );
}

#[test]
fn enabled_preview_receives_events_without_items_or_delivered_dimensions() {
    let mut model = model([]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    session.events().preview(PreviewEvent::Refresh);
    drop(session);
    assert!(probe.calls.contains(&Call::Preview(None, None, true)));
}

#[test]
fn idle_completion_retains_invalidations_and_abandoned_frames_require_a_full_repaint() {
    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe {
        after_redraw: vec![Components::NONE, Components::PREVIEW].into_iter(),
        ..Probe::default()
    };
    let mut session = model.session(&mut probe);
    let initial = session.prepare_frame(frame_options());
    assert!(initial.redraw().is_all());
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
    drop(session);
    assert_eq!(probe.hook_counts(), (4, 3));
}

#[test]
fn hidden_component_invalidations_are_acknowledged_by_completion() {
    let mut model = model(["alpha"]);
    let mut probe = Probe {
        after_redraw: vec![Components::ALL].into_iter(),
        ..Probe::default()
    };
    let mut session = model.session(&mut probe);
    session.events().set_active_components(Components::NONE);
    session.prepare_frame(frame_options()).complete();

    let hidden = session.prepare_frame(frame_options());
    assert!(hidden.changed());
    assert!(hidden.redraw().is_none());
    hidden.complete();

    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
}

#[test]
fn visibility_changes_persist_and_request_redraw_until_completed() {
    let mut model = model(["alpha"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    assert!(session.active_components().is_all());
    session.prepare_frame(frame_options()).complete();

    for (active, redraw) in [
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
                match_list: false,
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
                match_list: true,
                ..Components::NONE
            },
        ),
        (Components::NONE, Components::NONE),
        (Components::ALL, Components::ALL),
    ] {
        {
            let mut batch = session.events();
            batch.set_active_components(active);
            assert_eq!(batch.active_components(), active);
        }
        session.update_matches(Duration::ZERO);
        let frame = session.prepare_frame(frame_options());
        assert!(frame.changed());
        assert_eq!(frame.active_components(), active);
        assert_eq!(frame.redraw(), redraw);
        frame.complete();

        session.events().set_active_components(active);
        let idle = session.prepare_frame(frame_options());
        assert!(!idle.changed());
        idle.complete();
    }
}

#[test]
fn unsupported_preview_is_excluded_without_requesting_a_redraw() {
    let mut model = model(["alpha"]);
    let mut session = model.session(Select);
    let supported = Components {
        preview: false,
        ..Components::ALL
    };
    assert_eq!(session.active_components(), supported);
    session.prepare_frame(frame_options()).complete();
    {
        let mut batch = session.events();
        batch.set_active_components(Components::ALL);
        assert_eq!(batch.active_components(), supported);
    }
    let frame = session.prepare_frame(frame_options());
    assert!(!frame.changed());
    assert_eq!(frame.active_components(), supported);
    frame.complete();

    session.events().set_active_components(Components::PREVIEW);
    let hidden = session.prepare_frame(frame_options());
    assert!(hidden.changed());
    assert!(hidden.active_components().is_none());
    hidden.complete();
    session.events().set_active_components(Components::PREVIEW);
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
}

#[test]
fn hidden_components_do_not_paint_nonempty_targets_or_expose_a_cursor() {
    let mut model = model(["alpha"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    for active in [Components::NONE, Components::ALL] {
        session.events().set_active_components(active);
        let mut frame = session.prepare_frame(frame_options());
        let mut surfaces: [TestSurface; 4] = std::array::from_fn(|_| TestSurface::new(20, 5));
        let [prompt, list, status, preview] = &mut surfaces;
        frame.prompt().draw(prompt).unwrap();
        frame.match_list().draw(list).unwrap();
        frame.status().draw(status).unwrap();
        frame.preview().draw(preview).unwrap();
        for surface in surfaces {
            assert_eq!(!surface.touched, active.is_none());
        }
        assert_eq!(frame.prompt().cursor().is_some(), active.prompt);
        frame.complete();
    }
}

#[test]
fn content_invalidations_coalesce_across_batches_until_frame_completion() {
    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
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

    let frame = session.prepare_frame(frame_options());
    assert_eq!(frame.redraw(), Components::PREVIEW);
    frame.complete();
    let idle = session.prepare_frame(frame_options());
    assert!(!idle.changed());
    idle.complete();
}

#[test]
fn a_failed_paint_can_be_retried_without_repeating_frame_hooks_or_preview_resize() {
    struct FailOnce {
        surface: TestSurface,
        fail: bool,
    }

    impl Surface for FailOnce {
        type Error = io::Error;

        fn width(&self) -> u16 {
            self.surface.width()
        }

        fn height(&self) -> u16 {
            self.surface.height()
        }

        fn begin_row(&mut self, row: u16, clear: bool) -> io::Result<()> {
            if std::mem::take(&mut self.fail) {
                return Err(io::Error::other("paint failed"));
            }
            self.surface.begin_row(row, clear).unwrap();
            Ok(())
        }

        fn styled_text<'a>(
            &mut self,
            text: &'a str,
            spans: impl Iterator<Item = StyledStr<'a>>,
        ) -> io::Result<()> {
            self.surface.styled_text(text, spans).unwrap();
            Ok(())
        }

        fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> io::Result<()> {
            self.surface.ascii(text, style).unwrap();
            Ok(())
        }

        fn spaces(&mut self, width: u16, style: Style) -> io::Result<()> {
            self.surface.spaces(width, style).unwrap();
            Ok(())
        }
    }

    let mut model = model(["alpha", "beta"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    let mut frame = session.prepare_frame(frame_options());
    let mut surface = FailOnce {
        surface: TestSurface::new(13, 7),
        fail: true,
    };
    assert!(frame.preview().draw(&mut surface).is_err());
    frame.preview().draw(&mut surface).unwrap();
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
        let mut model = model(["alpha"]);
        let mut probe = Probe::default();
        let mut session = model.session(&mut probe);
        for (width, height) in [
            (13, 7),
            (13, 7),
            (0, 0),
            (0, 0),
            (13, 7),
            (0, 7),
            (13, 0),
            (13, 7),
        ] {
            let size = Dimensions { width, height };
            let mut frame = session.prepare_frame(frame_options());
            if draw {
                let mut surface = TestSurface::new(width, height);
                frame.preview().draw(&mut surface).unwrap();
                if width == 0 || height == 0 {
                    assert!(!surface.touched);
                }
            } else {
                frame.preview().resize(size);
            }
            frame.complete();
        }
        {
            let mut batch = session.events();
            batch.set_active_components(Components::NONE);
            batch.set_active_components(Components::ALL);
        }
        let mut frame = session.prepare_frame(frame_options());
        assert!(frame.changed());
        frame.preview().resize(Dimensions {
            width: 13,
            height: 7,
        });
        frame.complete();
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
            [(13, 7), (0, 0), (13, 7), (0, 7), (13, 0), (13, 7)]
                .map(|(width, height)| Some(Dimensions { width, height })),
            "draw={draw}",
        );
    }
}

#[test]
fn prepared_preview_delivers_content_sizes_for_bordered_targets() {
    let mut model = model(["alpha"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    for (width, height, active) in [
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
        session.events().set_active_components(Components {
            preview: active,
            ..Components::ALL
        });
        let mut frame = session.prepare_frame(frame_options());
        frame
            .preview()
            .border(BoundaryChars::new())
            .resize(Dimensions { width, height });
        frame.complete();
    }
    let mut frame = session.prepare_frame(frame_options());
    frame
        .preview()
        .border(BoundaryChars::new())
        .draw(&mut TestSurface::new(13, 7))
        .unwrap();
    frame.complete();
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
    );
}

#[test]
fn prepared_preview_border_changes_do_not_invalidate_or_repeat_content_resize() {
    let mut model = model(["alpha"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions {
        width: 13,
        height: 7,
    });
    initial.complete();
    for (width, height, border) in [
        (15, 9, Some(BoundaryChars::new())),
        (15, 9, Some(BoundaryChars::ascii())),
        (13, 7, None),
    ] {
        let mut frame = session.prepare_frame(frame_options());
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
    let mut model = model(["alpha"]);
    let mut probe = Probe::default();
    let mut session = model.session(&mut probe);
    for enabled in [false, true] {
        session.events().set_active_components(Components {
            preview: enabled,
            ..Components::ALL
        });
        let mut frame = session.prepare_frame(frame_options());
        frame.preview().resize(Dimensions::default());
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
        [None, Some(Dimensions::default())],
    );
}

#[test]
fn size_only_prompt_preparation_sets_local_cursor_and_subsequent_paint() {
    let mut model = model(["alpha"]);
    let mut session = model.session(Select);
    session
        .events()
        .prompt(PromptEvent::Reset("ＡＡＡＡＡ".to_owned()));
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(frame.prompt().cursor(), None);
    for (width, height, painted, column) in [
        (9, 1, ">  ＡＡ", Some(7)),
        (0, 1, "", None),
        (20, 0, "", None),
        (20, 1, "> ＡＡＡＡＡ", Some(12)),
    ] {
        frame.prompt().resize(Dimensions { width, height });
        assert_eq!(
            frame.prompt().cursor(),
            column.map(|column| Position { column, row: 0 })
        );
        let mut target = TestSurface::new(width, height);
        frame.prompt().draw(&mut target).unwrap();
        assert_eq!(target.touched, width != 0 && height != 0);
        if height != 0 {
            assert_eq!(target.rows, [painted]);
        }
        assert_eq!(
            frame.prompt().cursor(),
            column.map(|column| Position { column, row: 0 })
        );
    }
    frame.complete();
}

#[test]
fn grapheme_width_is_used_for_prompt_events_between_frames() {
    let mut model = model(["alpha"]);
    let mut session = model.session(Select);
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

    use crate::draw::RatatuiSurface;

    for draw_first in [false, true] {
        let mut model = model(["alpha"]);
        let mut session = model.session(Select);
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
        let mut model = model(["alpha", "beta"]);
        let mut probe = Probe::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut session = model.session(&mut probe);
            let mut batch = session.events();
            batch.prompt(PromptEvent::Insert('x'));
            batch.match_list(MatchListEvent::Up(1));
            assert!(!panic, "unwind with an active batch");
        }));
        assert_eq!(result.is_err(), panic);
        assert_eq!(model.query(), "x");
        assert_eq!(
            probe
                .calls
                .iter()
                .filter(|call| **call == Call::End)
                .count(),
            usize::from(!panic)
        );
        assert!(!probe.calls.contains(&Call::Finish));
        assert!(model.filter().unwrap().is_empty());
    }
}
