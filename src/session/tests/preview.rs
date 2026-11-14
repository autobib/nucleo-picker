use std::{
    cell::Cell,
    collections::VecDeque,
    io,
    rc::Rc,
    time::{Duration, Instant},
};

use super::{FrameEvents, frame_events};
use crate::{
    PickerOptions, Terminal,
    component::Component,
    error::PickError,
    event::{Event, EventSource, LayoutEvent, MatchListEvent, RecvError},
    preview::{
        Preview, PreviewBuffer, PreviewEvent, PreviewPane, SyncPreviewer,
        controller::{PreviewController, tests::support::*},
        request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
    },
    rect::Area,
    render::StrRenderer,
};

#[derive(Default)]
struct TestTerminal {
    output: Vec<u8>,
    cleanups: usize,
    sizes: VecDeque<(u16, u16)>,
    size_override: Option<Rc<Cell<(u16, u16)>>>,
    render_count: Option<Rc<Cell<usize>>>,
    changed: Vec<bool>,
    fail_write: bool,
    fail_cleanup: bool,
}

impl io::Write for TestTerminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            return Err(io::Error::other("write failed"));
        }
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(count) = &self.render_count {
            count.set(count.get() + 1);
        }
        Ok(())
    }
}

impl Terminal for TestTerminal {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.cleanups += 1;
        if self.fail_cleanup {
            return Err(io::Error::other("cleanup failed"));
        }
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        if let Some(size) = &self.size_override {
            Ok(size.get())
        } else if self.sizes.len() > 1 {
            Ok(self.sizes.pop_front().unwrap())
        } else {
            Ok(self.sizes.front().copied().unwrap_or((20, 10)))
        }
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.changed.push(changed);
        Ok(())
    }
}

fn offset(
    session: &mut PreviewController<&'static str, TestPreviewer>,
    idx: u32,
) -> (usize, usize) {
    session
        .test_cache()
        .peek(&idx)
        .unwrap()
        .pane()
        .unwrap()
        .offset()
}

#[test]
fn prefetch_follows_rendering_and_progresses_on_idle_frames() {
    struct Previewer {
        render_count: Rc<Cell<usize>>,
        prefetched: Vec<(&'static str, usize)>,
    }

    impl Preview<&'static str> for Previewer {
        fn prefetch_limit(&self) -> usize {
            3
        }

        fn preview(
            &mut self,
            request: PreviewRequest<'_, &'static str>,
            _: Duration,
        ) -> PreviewResponse {
            let item = request.item();
            let mut pane = request.ready();
            pane.buffer_mut().push_str(item);
            PreviewResponse::Ready(pane)
        }

        fn prefetch(
            &mut self,
            request: PreviewRequest<'_, &'static str>,
            timeout: Duration,
        ) -> Result<PreviewResponse, PreviewBuffer> {
            self.prefetched
                .push((*request.item(), self.render_count.get()));
            Ok(self.preview(request, timeout))
        }
    }

    let render_count = Rc::new(Cell::new(0));
    let mut previewer = Previewer {
        render_count: Rc::clone(&render_count),
        prefetched: Vec::new(),
    };
    let mut terminal = TestTerminal {
        render_count: Some(render_count),
        ..TestTerminal::default()
    };
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    picker.push_batch(["a", "b", "c", "d"]);
    settle(&mut picker);
    let events = frame_events([
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]);
    picker
        .with_preview(&mut previewer)
        .pick_with_terminal_io(events, &mut terminal)
        .unwrap();

    assert_eq!(previewer.prefetched, [("b", 1), ("c", 1), ("d", 2)]);
    assert_eq!(terminal.changed, [false, true]);
}

#[test]
fn buffered_navigation_scroll_and_refresh_target_the_event_hovered_item() {
    for (case, reversed, cached) in [
        ("cached", false, true),
        ("cached, reversed", true, true),
        ("missing", false, false),
        ("missing, reversed", true, false),
    ] {
        let mut picker = PickerOptions::new()
            .reverse_items(false)
            .reversed(reversed)
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        settle(&mut picker);
        let mut session = preview_pane(TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        });
        if cached {
            session.update_at(Some(1), &picker.core.engine, Instant::now());
        }
        let next = if reversed {
            MatchListEvent::Down
        } else {
            MatchListEvent::Up
        };
        let previous = if reversed {
            MatchListEvent::Up
        } else {
            MatchListEvent::Down
        };
        let events = frame_events([
            Ok(Event::Preview(PreviewEvent::Up(2))),
            Ok(Event::Preview(PreviewEvent::Down(2))),
            Ok(Event::MatchList(next(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Ok(Event::Preview(PreviewEvent::Up(1))),
            Ok(Event::MatchList(previous(1))),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        picker
            .core
            .pick_impl(events, &mut TestTerminal::default(), (), &mut session)
            .unwrap();
        assert_eq!(offset(&mut session, 0), (0, 3), "{case}");
        if cached {
            assert_eq!(offset(&mut session, 1), (0, 7), "{case}");
        } else {
            assert!(!session.test_cache().contains(&1), "{case}");
        }
        assert_eq!(
            session.test_previewer().requested,
            if cached {
                &["beta", "alpha"][..]
            } else {
                &["alpha"][..]
            },
            "{case}"
        );

        let events = frame_events([
            Ok(Event::MatchList(next(1))),
            Ok(Event::Preview(PreviewEvent::Refresh)),
            Ok(Event::MatchList(previous(1))),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(next(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        let mut terminal = TestTerminal::default();
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        assert_eq!(offset(&mut session, 0), (0, 3), "{case}");
        assert_eq!(offset(&mut session, 1), (0, 0), "{case}");
        assert_eq!(
            session.test_previewer().requested,
            if cached {
                &["beta", "alpha", "beta"][..]
            } else {
                &["alpha", "beta"][..]
            },
            "{case}"
        );
        assert_eq!(terminal.changed, [false, true], "{case}");
    }
}

#[test]
fn refresh_does_not_run_the_previewer_before_a_following_quit() {
    for defer in [false, true] {
        let picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer,
            ..TestPreviewer::default()
        };
        let events = frame_events([Ok(Event::Preview(PreviewEvent::Refresh)), Ok(Event::Quit)]);
        assert!(
            picker
                .with_preview(&mut previewer)
                .pick_with_terminal_io(events, &mut TestTerminal::default())
                .unwrap()
                .is_none()
        );
        assert_eq!(previewer.requested, ["alpha"], "defer={defer}");
        let cancelled = previewer
            .queued
            .iter()
            .all(QueuedPreviewRequest::is_cancelled);
        assert!(cancelled, "defer={defer}");
    }
}

#[test]
fn scrolling_uses_item_ids_and_only_redraws_the_preview() {
    for (case, items, query, idx, item) in [
        ("unfiltered", &["alpha"][..], "", 0, "alpha"),
        ("filtered", &["alpha", "beta"][..], "beta", 1, "beta"),
    ] {
        let mut picker = picker(items.iter().copied());
        picker.update_query(query);
        settle(&mut picker);
        let mut session = preview_pane(TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        });
        let mut terminal = TestTerminal::default();
        let events = frame_events([
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        assert_eq!(offset(&mut session, idx), (0, 8), "{case}");
        assert!(!session.test_cache().contains(&(1 - idx)), "{case}");
        assert_eq!(terminal.changed, [true], "{case}");
        let output = String::from_utf8(terminal.output).unwrap();
        if query.is_empty() {
            assert_eq!(output.matches(item).count(), 17, "{case}");
        }
        assert_eq!(output.matches('▌').count(), 1, "{case}");
        assert_eq!(output.matches('>').count(), 1, "{case}");
        assert_eq!(output.matches('╭').count(), 2, "{case}");

        let events = frame_events([
            Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        picker
            .core
            .pick_impl(events, &mut TestTerminal::default(), (), &mut session)
            .unwrap();
        assert_eq!(
            offset(&mut session, idx),
            (0, 21),
            "{case}: bottom saturation"
        );
        assert_eq!(session.test_previewer().requested, [item], "{case}");
    }
}

#[test]
fn pending_navigation_and_scrolling_wait_for_idle_completion() {
    for size in [(20, 10), (30, 12)] {
        let mut picker = picker(["alpha", "beta"]);
        let status = picker.status_observer();
        let mut session = preview_pane(TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        });
        let events = frame_events([
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        let mut terminal = TestTerminal::default();
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        assert!(
            session.test_cache().peek(&0).unwrap().pane().is_none(),
            "{size:?}"
        );
        assert!(!session.test_cache().contains(&1), "{size:?}");
        let previewer = session.test_previewer();
        assert_eq!(previewer.requested, ["alpha"], "{size:?}");
        assert_eq!(previewer.hovered, [Some("alpha")], "{size:?}");
        assert!(!previewer.queued[0].is_cancelled(), "{size:?}");
        assert_eq!(terminal.changed, [false], "{size:?}");

        let mut queued = Some(session.test_previewer().take_request("alpha"));
        let mut step = 0;
        let events = FrameEvents(move || {
            step += 1;
            match step {
                1 | 3 | 5 | 6 => Err(RecvError::Timeout),
                2 => {
                    let mut buffer = PreviewBuffer::new();
                    for _ in 1..30 {
                        buffer.newline();
                    }
                    let active = queued.take().unwrap().start().unwrap();
                    active.publish(&mut buffer.into_pane()).unwrap();
                    Ok(Event::Status { id: 0 })
                }
                4 => Ok(Event::Preview(PreviewEvent::Down(1))),
                _ => Ok(Event::Quit),
            }
        });
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([(20, 10), size]),
            ..TestTerminal::default()
        };
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        let status = status.try_recv().unwrap();
        assert_eq!((status.width, status.height), size);
        assert_eq!(terminal.changed, [false, true, true, false], "{size:?}");
        assert_eq!(offset(&mut session, 0), (0, 1), "{size:?}");
        assert_eq!(session.test_previewer().requested, ["alpha"], "{size:?}");

        let output = String::from_utf8(terminal.output).unwrap();
        let list_draws = 1 + usize::from(size != (20, 10));
        assert_eq!(output.matches("alpha").count(), list_draws, "{size:?}");
        assert_eq!(output.matches('>').count(), list_draws, "{size:?}");
        assert_eq!(output.matches('╭').count(), 3, "{size:?}");
    }
}

#[test]
fn redraw_checks_terminal_size_after_preview_generation() {
    let mut picker = picker(["alpha", "beta"]);
    let status = picker.status_observer();
    let size = Rc::new(Cell::new((20, 10)));
    let mut terminal = TestTerminal {
        size_override: Some(Rc::clone(&size)),
        ..TestTerminal::default()
    };
    let previewer = SyncPreviewer(|item: &&str, pane: &mut PreviewPane| {
        let buffer = pane.buffer_mut();
        if *item == "beta" {
            size.set((30, 12));
        }
        buffer.push_str(item);
    });
    let events = frame_events([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::Status { id: 0 }),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]);
    picker
        .with_preview(previewer)
        .pick_with_terminal_io(events, &mut terminal)
        .unwrap();
    let status = status.try_recv().unwrap();
    assert_eq!((status.width, status.height), (30, 12));
}

#[test]
fn resizing_retained_previews_preserves_hover_and_updates_page_height() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    update(&mut session, &mut picker);
    session.resize(
        Area {
            width: 22,
            height: 6,
            ..Area::default()
        },
        &(),
    );
    session.handle_at(PreviewEvent::Down(usize::MAX), Some(0));
    assert_eq!(offset(&mut session, 0), (0, 26));

    picker
        .core
        .pick_impl(
            frame_events([Ok(Event::Quit)]),
            &mut TestTerminal::default(),
            (),
            &mut session,
        )
        .unwrap();
    assert_eq!(offset(&mut session, 0), (0, 22), "initial layout");

    for (case, sizes, scroll, expected) in [
        ("narrow", [(20, 10), (5, 10)], PreviewEvent::Down(1), 22),
        ("restore", [(5, 10), (20, 10)], PreviewEvent::Down(1), 22),
        ("grow", [(20, 10), (20, 14)], PreviewEvent::PageUp(1), 6),
    ] {
        let mut terminal = TestTerminal {
            sizes: sizes.into(),
            ..TestTerminal::default()
        };
        let events = frame_events([
            Ok(Event::Redraw),
            Ok(Event::Preview(scroll)),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        assert_eq!(offset(&mut session, 0), (0, expected), "{case}");
        let previewer = session.test_previewer();
        assert_eq!(previewer.hovered, [Some("alpha")], "{case}");
        assert_eq!(previewer.requested, ["alpha"], "{case}");
    }
}

#[test]
fn resizing_clamps_cached_previews_before_they_are_revisited() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    update(&mut session, &mut picker);
    session.resize(
        Area {
            width: 22,
            height: 10,
            ..Area::default()
        },
        &(),
    );
    session.handle_at(PreviewEvent::Down(usize::MAX), Some(0));
    assert_eq!(offset(&mut session, 0), (0, 22));
    picker.core.list_state.layout.set_rank(
        picker.core.engine.snapshot(),
        1,
        &picker.core.list_state.config,
    );
    let mut terminal = TestTerminal {
        sizes: VecDeque::from([(20, 10), (20, 14)]),
        ..TestTerminal::default()
    };
    picker
        .core
        .pick_impl(
            frame_events([Ok(Event::Redraw), Err(RecvError::Timeout), Ok(Event::Quit)]),
            &mut terminal,
            (),
            &mut session,
        )
        .unwrap();
    assert_eq!(offset(&mut session, 0), (0, 18));
    picker
        .core
        .pick_impl(
            frame_events([
                Ok(Event::MatchList(MatchListEvent::Down(1))),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]),
            &mut terminal,
            (),
            &mut session,
        )
        .unwrap();
    assert_eq!(offset(&mut session, 0), (0, 18));
    assert_eq!(session.test_previewer().requested, ["alpha", "beta"]);
}

#[test]
fn zero_sized_panes_ignore_scrolling() {
    for size in [(0, 10), (5, 10), (20, 2), (20, 0)] {
        let mut picker = picker(["alpha"]);
        let mut session = preview_pane(TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        });
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([size]),
            ..TestTerminal::default()
        };
        let events = frame_events([
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        picker
            .core
            .pick_impl(events, &mut terminal, (), &mut session)
            .unwrap();
        assert_eq!(offset(&mut session, 0), (0, 0), "{size:?}");
        let previewer = session.test_previewer();
        assert_eq!(previewer.hovered, [Some("alpha")], "{size:?}");
        assert_eq!(previewer.requested, ["alpha"], "{size:?}");
        assert_eq!(terminal.changed, [false], "{size:?}");
    }
}

fn navigation() -> impl EventSource<AbortErr = &'static str> {
    frame_events([
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Ok(Event::Preview(PreviewEvent::Refresh)),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Select),
    ])
}

#[test]
fn preview_picker_round_trips_state_and_preserves_selection_modes() {
    let mut picker = picker(["alpha", "beta"]);
    let mut plain_terminal = TestTerminal::default();
    let selected = picker
        .pick_with_terminal_io(navigation(), &mut plain_terminal)
        .unwrap();
    assert_eq!(selected, Some(&"alpha"));
    assert_eq!(plain_terminal.changed, [false, true, true]);
    assert!(
        !String::from_utf8(plain_terminal.output)
            .unwrap()
            .contains('╭')
    );

    let mut preview_picker = picker.with_preview(TestPreviewer::default());
    let mut terminal = TestTerminal::default();
    let selected = preview_picker
        .pick_with_terminal_io(navigation(), &mut terminal)
        .unwrap();
    assert_eq!(selected, Some(&"alpha"));
    assert_eq!(
        preview_picker.previewer().requested,
        ["alpha", "alpha", "beta"]
    );
    assert_eq!(terminal.changed, [true, true, true]);
    assert!(String::from_utf8(terminal.output).unwrap().contains('╭'));
    assert_eq!(terminal.cleanups, 1);

    let selected = preview_picker
        .as_multi()
        .pick_with_terminal_io(
            frame_events([
                Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                Err(RecvError::Timeout),
                Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                Ok(Event::Select),
            ]),
            &mut TestTerminal::default(),
        )
        .unwrap();
    assert!(selected.iter().copied().eq(["alpha", "beta"]));

    let injector = preview_picker.injector();
    preview_picker.update_query("beta");
    preview_picker.previewer_mut().lines = 3;
    let requests = preview_picker.previewer().requested.len();
    let callbacks = preview_picker.previewer().hovered.len();
    settle(&mut preview_picker);
    for (case, exit, expected) in [
        ("select", Event::Select, Some("beta")),
        ("quit", Event::Quit, None),
    ] {
        let selected = preview_picker
            .pick_with_terminal_io(frame_events([Ok(exit)]), &mut TestTerminal::default())
            .unwrap();
        assert_eq!(selected.copied(), expected, "{case}");
    }
    let (mut picker, previewer) = preview_picker.into_parts();
    assert_eq!(picker.query(), "beta");
    assert_eq!(previewer.lines, 3);
    assert_eq!(previewer.requested[requests..], ["beta", "beta"]);
    assert_eq!(previewer.ids[requests], previewer.ids[requests + 1]);
    assert_eq!(
        previewer.hovered[callbacks..],
        [Some("beta"), None, Some("beta"), None]
    );

    injector.push("gamma");
    picker.reset_query();
    settle(&mut picker);
    let selected = picker
        .as_multi()
        .pick_with_terminal_io(
            frame_events([
                Ok(Event::MatchList(MatchListEvent::QueueMatches)),
                Ok(Event::Select),
            ]),
            &mut TestTerminal::default(),
        )
        .unwrap();
    assert!(selected.iter().copied().eq(["alpha", "beta", "gamma"]));
}

#[test]
fn picker_exits_cancel_all_pending_requests() {
    for (case, exit, succeeds) in [
        ("select", Ok(Event::Select), true),
        ("quit", Ok(Event::Quit), true),
        ("quit empty", Ok(Event::QuitPromptEmpty), true),
        ("interrupt", Ok(Event::UserInterrupt), false),
        ("abort", Ok(Event::Abort("aborted")), false),
        ("restart", Ok(Event::Restart), false),
        ("disconnect", Err(RecvError::Disconnected), false),
        (
            "read error",
            Err(RecvError::IO(io::Error::other("read failed"))),
            false,
        ),
    ] {
        let picker = picker(["alpha", "beta"]);
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        let events = frame_events([
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            exit,
        ]);
        let mut terminal = TestTerminal::default();
        let mut preview_picker = picker.with_preview(&mut previewer);
        let result = preview_picker.pick_with_terminal_io(events, &mut terminal);

        assert_eq!(result.is_ok(), succeeds, "{case}");
        assert_eq!(terminal.cleanups, 1, "{case}");
        assert_eq!(previewer.queued.len(), 2, "{case}");
        let cancelled = previewer
            .queued
            .iter()
            .all(QueuedPreviewRequest::is_cancelled);
        assert!(cancelled, "{case}");
        let hovered = [Some("alpha"), Some("beta"), None];
        assert_eq!(previewer.hovered, hovered, "{case}");
        assert_eq!(previewer.cancelled_on_clear, [true], "{case}");
    }
}

#[test]
fn terminal_errors_cancel_pending_requests() {
    for (case, fail_write) in [("write", true), ("cleanup", false)] {
        let picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        let mut terminal = TestTerminal {
            fail_write,
            fail_cleanup: !fail_write,
            ..TestTerminal::default()
        };
        let mut preview_picker = picker.with_preview(&mut previewer);
        let result =
            preview_picker.pick_with_terminal_io(frame_events([Ok(Event::Quit)]), &mut terminal);

        assert!(matches!(result, Err(PickError::IO(_))), "{case}");
        assert_eq!(terminal.cleanups, 1, "{case}");
        assert!(previewer.queued[0].is_cancelled(), "{case}");
        assert_eq!(previewer.hovered, [Some("alpha"), None], "{case}");
        assert_eq!(previewer.cancelled_on_clear, [true], "{case}");
    }
}

#[test]
fn unwinding_cancels_pending_requests() {
    let picker = picker(["alpha"]);
    let mut previewer = TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    };
    let mut terminal = TestTerminal::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut preview_picker = picker.with_preview(&mut previewer);
        let _ = preview_picker
            .pick_with_terminal_io(FrameEvents(|| panic!("event source panic")), &mut terminal);
    }));

    assert!(panic.is_err());
    assert_eq!(terminal.cleanups, 1);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.hovered, [Some("alpha")]);
    assert!(previewer.cancelled_on_clear.is_empty());
}

#[test]
fn replacing_items_between_sessions_changes_ids() {
    let mut previewer = TestPreviewer::default();
    let mut preview_picker = picker(["alpha"]).with_preview(&mut previewer);
    for step in 0..3 {
        let picker = &mut preview_picker;
        match step {
            0 => {}
            1 => picker.restart(),
            2 => picker.reset_renderer(StrRenderer),
            _ => unreachable!(),
        }
        if step != 0 {
            picker.push_batch(["alpha"]);
            settle(picker);
        }
        preview_picker
            .pick_with_terminal_io(
                frame_events([Ok(Event::Quit)]),
                &mut TestTerminal::default(),
            )
            .unwrap();
    }
    assert_eq!(previewer.requested, ["alpha", "alpha", "alpha"]);
    let ids = &previewer.ids;
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
    assert_ne!(ids[1], ids[2]);
}

#[test]
fn restart_invalidates_cached_item_ids() {
    let mut picker = picker(["alpha"]);
    let injectors = picker.injector_observer(false);
    let statuses = picker.status_observer();
    let mut step = 0;
    let started = Instant::now();
    let events = FrameEvents(|| {
        assert!(started.elapsed() < Duration::from_secs(5));
        match step {
            0 => {
                step = 1;
                Ok(Event::Restart)
            }
            1 => {
                injectors.try_recv().unwrap().push("beta");
                step = 2;
                Ok(Event::Status { id: 0 })
            }
            2 => {
                step = 3;
                Err(RecvError::Timeout)
            }
            _ => {
                if statuses.try_recv().unwrap().matched_item_count == 1 {
                    Ok(Event::Select)
                } else {
                    step = 2;
                    Ok(Event::Status { id: 0 })
                }
            }
        }
    });
    let mut previewer = TestPreviewer::default();
    let mut preview_picker = picker.with_preview(&mut previewer);
    let selected = preview_picker
        .pick_with_terminal_io(events, &mut TestTerminal::default())
        .unwrap();
    assert_eq!(selected, Some(&"beta"));
    assert_eq!(previewer.requested, ["alpha", "beta"]);
    assert_eq!(previewer.hovered, [Some("alpha"), None, Some("beta"), None]);
    assert_ne!(previewer.ids[0], previewer.ids[1]);
    assert_eq!(previewer.restarts, 1);
}

#[test]
fn hide_reveal_coalesces_navigation_and_only_resubmits_queued_previews() {
    for (case, items, defer, size) in [
        ("ready", &["alpha", "beta"][..], false, (20, 10)),
        ("queued", &["alpha", "beta"][..], true, (20, 10)),
        ("ready, small pane", &["alpha", "beta"][..], false, (5, 10)),
        ("queued, small pane", &["alpha", "beta"][..], true, (5, 10)),
        ("empty", &[][..], true, (20, 10)),
    ] {
        let mut previewer = TestPreviewer {
            defer,
            ..TestPreviewer::default()
        };
        let events = frame_events([
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([size]),
            ..TestTerminal::default()
        };
        picker(items.iter().copied())
            .with_preview(&mut previewer)
            .pick_with_terminal_io(events, &mut terminal)
            .unwrap();
        if items.is_empty() {
            assert!(previewer.hovered.is_empty(), "{case}");
            assert!(previewer.requested.is_empty(), "{case}");
        } else {
            let hovered = ["alpha", "alpha", "beta", "alpha"]
                .into_iter()
                .flat_map(|item| [Some(item), None]);
            assert!(previewer.hovered.iter().copied().eq(hovered), "{case}");
            assert_eq!(
                previewer.requested,
                if defer {
                    &["alpha", "alpha", "beta", "alpha"][..]
                } else {
                    &["alpha", "beta"][..]
                },
                "{case}"
            );
            assert_eq!(previewer.cancelled_on_clear, [!defer; 4], "{case}");
        }
        let cancelled = previewer
            .queued
            .iter()
            .all(QueuedPreviewRequest::is_cancelled);
        assert!(cancelled, "{case}");
    }
}
