use std::{cell::Cell, collections::VecDeque, rc::Rc};

use crate::Picker;
use crate::{
    PickerOptions,
    component::{Component, PreviewComponent},
    error::PickError,
    event::{Event, EventSource, LayoutEvent},
    preview::{
        Preview, PreviewBuffer, PreviewEvent, SyncPreviewer, WithPreview,
        pane::tests::support::*,
        request::{PreviewRequest, PreviewResponse, QueuedPreviewRequest},
    },
    rect::Area,
    render::StrRenderer,
};
use crate::{
    Terminal,
    event::{MatchListEvent, RecvError},
};
use std::{
    io,
    time::{Duration, Instant},
};

struct Events(VecDeque<Result<Event<&'static str>, RecvError>>);

impl EventSource for Events {
    type AbortErr = &'static str;

    fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
        self.0.pop_front().expect("event script exhausted")
    }
}

struct CallbackEvents<F>(F);

impl<F: FnMut() -> Result<Event<&'static str>, RecvError>> EventSource for CallbackEvents<F> {
    type AbortErr = &'static str;

    fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
        (self.0)()
    }
}

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
            let mut buffer = request.ready();
            buffer.push_str(item);
            PreviewResponse::Ready(buffer)
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
    let events = Events(VecDeque::from([
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .with_preview(&mut previewer)
        .pick_with_terminal_io(events, &mut terminal)
        .unwrap();

    assert_eq!(previewer.prefetched, [("b", 1), ("c", 1), ("d", 2)]);
    assert_eq!(terminal.changed, [false, true]);
}

#[test]
fn interleaved_navigation_and_scrolling_target_the_buffered_item() {
    for reversed in [false, true] {
        let mut picker = PickerOptions::new()
            .reverse_items(false)
            .reversed(reversed)
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        settle(&mut picker);
        picker.core.list_state.layout.resize(
            picker.core.engine.snapshot(),
            8,
            &picker.core.list_state.config,
        );
        let mut session = preview_pane(TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        });
        picker.core.list_state.layout.set_selection(
            picker.core.engine.snapshot(),
            1,
            &picker.core.list_state.config,
        );
        update(&mut session, &mut picker);
        picker.core.list_state.layout.set_selection(
            picker.core.engine.snapshot(),
            0,
            &picker.core.list_state.config,
        );
        let (next, previous) = if reversed {
            (MatchListEvent::Down(1), MatchListEvent::Up(1))
        } else {
            (MatchListEvent::Up(1), MatchListEvent::Down(1))
        };
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Up(2))),
            Ok(Event::Preview(PreviewEvent::Down(2))),
            Ok(Event::MatchList(next)),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Ok(Event::Preview(PreviewEvent::Up(1))),
            Ok(Event::MatchList(previous)),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .core
            .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
            .unwrap();
        assert_eq!(
            session
                .test_cache()
                .peek(&0)
                .unwrap()
                .buffer()
                .unwrap()
                .offset()
                .1,
            3
        );
        assert_eq!(
            session
                .test_cache()
                .peek(&1)
                .unwrap()
                .buffer()
                .unwrap()
                .offset()
                .1,
            7
        );
        assert_eq!(session.test_previewer().requested, ["beta", "alpha"]);
    }
}

#[test]
fn batched_navigation_and_refresh_target_the_event_selection() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer::default());
    session.update(Some(1), &picker.core.engine, Instant::now());
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::Preview(PreviewEvent::Refresh)),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    let mut terminal = TestTerminal::default();
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session.test_previewer().requested,
        ["beta", "alpha", "beta"]
    );
    assert_eq!(terminal.changed, [false, true]);
}

#[test]
fn refresh_does_not_run_the_previewer_before_a_following_quit() {
    for defer in [false, true] {
        let picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer,
            ..TestPreviewer::default()
        };
        let mut terminal = TestTerminal::default();
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Refresh)),
            Ok(Event::Quit),
        ]));
        let mut preview_picker = picker.with_preview(&mut previewer);
        assert!(
            preview_picker
                .pick_with_terminal_io(events, &mut terminal)
                .unwrap()
                .is_none()
        );
        assert_eq!(previewer.requested, ["alpha"]);
        assert!(
            previewer
                .queued
                .iter()
                .all(QueuedPreviewRequest::is_cancelled)
        );
    }
}

#[test]
fn scrolling_a_filtered_match_uses_its_item_id() {
    let mut picker = picker(["alpha", "beta"]);
    picker.update_query("beta");
    settle(&mut picker);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&1)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        8
    );
    assert!(!session.test_cache().contains(&0));
}

#[test]
fn scrolling_a_missing_preview_does_not_request_it() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset(),
        (0, 0)
    );
    assert!(!session.test_cache().contains(&1));
}

#[test]
fn scrolling_pending_previews_is_ignored_by_the_loop() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let mut terminal = TestTerminal::default();
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert!(session.test_cache().peek(&0).unwrap().buffer().is_none());
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(terminal.changed, [false]);
}

#[test]
fn idle_frames_collect_completed_previews_and_enable_scrolling() {
    for size in [(20, 10), (30, 12)] {
        let mut picker = picker(["alpha"]);
        let status = picker.status_observer();
        let mut session = preview_pane(TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        });
        update(&mut session, &mut picker);
        let mut queued = session.test_previewer().queued.pop();
        let mut step = 0;
        let events = CallbackEvents(move || {
            step += 1;
            match step {
                1 | 3 | 5 | 6 => Err(RecvError::Timeout),
                2 => {
                    let mut buffer = PreviewBuffer::new();
                    for _ in 1..30 {
                        buffer.newline();
                    }
                    assert!(
                        queued
                            .take()
                            .unwrap()
                            .start()
                            .unwrap()
                            .publish(&mut buffer)
                            .is_ok()
                    );
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
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        let status = status.try_recv().unwrap();
        assert_eq!((status.width, status.height), size);
        assert_eq!(terminal.changed, [false, true, true, false]);
        assert_eq!(
            session
                .test_cache()
                .peek(&0)
                .unwrap()
                .buffer()
                .unwrap()
                .offset()
                .1,
            1
        );
        assert_eq!(session.test_previewer().requested, ["alpha"]);

        let output = String::from_utf8(terminal.output).unwrap();
        let list_draws = 1 + usize::from(size != (20, 10));
        assert_eq!(output.matches("alpha").count(), list_draws);
        assert_eq!(output.matches('>').count(), list_draws);
        assert_eq!(output.matches('╭').count(), 3);
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
    let previewer = SyncPreviewer(|item: &&str, buffer: &mut PreviewBuffer| {
        if *item == "beta" {
            size.set((30, 12));
        }
        buffer.push_str(item);
    });
    let mut steps = VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::Status { id: 0 }),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]);
    let events = super::TestEvents(|timeout: Duration| {
        if timeout.is_zero() {
            Err(RecvError::Timeout)
        } else {
            steps.pop_front().unwrap()
        }
    });
    picker
        .with_preview(previewer)
        .pick_with_terminal_io(events, &mut terminal)
        .unwrap();
    let status = status.try_recv().unwrap();
    assert_eq!((status.width, status.height), (30, 12));
}

#[test]
fn batched_navigation_back_to_the_same_item_does_not_promote() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    let mut terminal = TestTerminal::default();
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(terminal.changed, [false]);
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(session.test_previewer().focused, [Some("alpha")]);
    assert!(!session.test_previewer().queued[0].is_cancelled());
}

#[test]
fn scroll_changes_are_reported_without_redrawing_the_match_list() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut terminal = TestTerminal::default();
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        8
    );
    assert_eq!(terminal.changed, [true]);

    let output = String::from_utf8(terminal.output).unwrap();
    assert_eq!(output.matches("alpha").count(), 17);
    assert_eq!(output.matches('▌').count(), 1);
    assert_eq!(output.matches('>').count(), 1);
    assert_eq!(output.matches('╭').count(), 2);
}

#[test]
fn resizing_the_pane_preserves_focus_and_reclamps_the_offset() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    update(&mut session, &mut picker);
    session
        .test_cache()
        .get_mut(&0)
        .unwrap()
        .buffer_mut()
        .unwrap()
        .set_offset(0, 25);
    let mut terminal = TestTerminal {
        sizes: VecDeque::from([(5, 10), (20, 10), (5, 10), (20, 10)]),
        ..TestTerminal::default()
    };
    let events = Events(VecDeque::from([
        Ok(Event::Redraw),
        Err(RecvError::Timeout),
        Ok(Event::Redraw),
        Err(RecvError::Timeout),
        Ok(Event::Redraw),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        22
    );
    assert_eq!(session.test_previewer().focused, [Some("alpha")]);
    assert_eq!(session.test_previewer().requested, ["alpha"]);
}

#[test]
fn resizing_reclamps_the_offset_and_changes_page_height() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut terminal = TestTerminal {
        sizes: VecDeque::from([(20, 10), (20, 14)]),
        ..TestTerminal::default()
    };
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
        Ok(Event::Redraw),
        Err(RecvError::Timeout),
        Ok(Event::Preview(PreviewEvent::PageUp(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        6
    );
}

#[test]
fn initial_layout_resizes_retained_preview_state() {
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
    session.handle(PreviewEvent::Down(usize::MAX), Some(0));
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        26
    );

    picker
        .core
        .pick_impl::<_, _, (), _>(
            Events(VecDeque::from([Ok(Event::Quit)])),
            &mut TestTerminal::default(),
            &mut session,
        )
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        22
    );
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
    session.handle(PreviewEvent::Down(usize::MAX), Some(0));
    picker.core.list_state.layout.set_selection(
        picker.core.engine.snapshot(),
        1,
        &picker.core.list_state.config,
    );
    let events = Events(VecDeque::from([
        Ok(Event::Redraw),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    let mut terminal = TestTerminal {
        sizes: VecDeque::from([(20, 10), (20, 14)]),
        ..TestTerminal::default()
    };
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        18
    );
    assert_eq!(session.test_previewer().requested, ["alpha", "beta"]);
}

#[test]
fn scrolling_down_down_up_at_the_bottom_moves_up_one_line() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .core
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(
        session
            .test_cache()
            .peek(&0)
            .unwrap()
            .buffer()
            .unwrap()
            .offset()
            .1,
        21
    );
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
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .core
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(
            session
                .test_cache()
                .peek(&0)
                .unwrap()
                .buffer()
                .unwrap()
                .offset(),
            (0, 0)
        );
        assert_eq!(session.test_previewer().focused, [Some("alpha")]);
        assert_eq!(session.test_previewer().requested, ["alpha"]);
        assert_eq!(terminal.changed, [false]);
    }
}

#[test]
fn ordinary_pickers_ignore_preview_events() {
    let mut picker = picker(["alpha"]);
    let mut terminal = TestTerminal::default();
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Ok(Event::Preview(PreviewEvent::Refresh)),
        Err(RecvError::Timeout),
        Ok(Event::Select),
    ]));
    let selected = picker.pick_with_terminal_io(events, &mut terminal).unwrap();
    assert_eq!(selected, Some(&"alpha"));
    assert_eq!(terminal.changed, [false]);
}

fn navigation() -> Events {
    Events(VecDeque::from([
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Select),
    ]))
}

#[test]
fn preview_picker_can_be_returned_and_recovered_with_its_state() {
    fn make_picker() -> Picker<&'static str, StrRenderer, WithPreview<crate::Single, TestPreviewer>>
    {
        picker(["alpha", "beta"]).with_preview(TestPreviewer::default())
    }

    let mut preview_picker = make_picker();
    let injector = preview_picker.injector();
    preview_picker.update_query("beta");
    preview_picker.previewer_mut().lines = 3;
    settle(&mut preview_picker);
    for (exit, expected) in [(Event::Select, Some("beta")), (Event::Quit, None)] {
        let selected = preview_picker
            .pick_with_terminal_io(
                Events(VecDeque::from([Ok(exit)])),
                &mut TestTerminal::default(),
            )
            .unwrap();
        assert_eq!(selected.copied(), expected);
    }

    let (mut picker, previewer) = preview_picker.into_parts();
    assert_eq!(picker.query(), "beta");
    assert_eq!(previewer.lines, 3);
    assert_eq!(previewer.requested, ["beta", "beta"]);
    assert_eq!(previewer.ids[0], previewer.ids[1]);
    assert_eq!(previewer.focused, [Some("beta"), None, Some("beta"), None]);

    injector.push("gamma");
    picker.reset_query();
    settle(&mut picker);
    let selected = picker
        .as_multi()
        .pick_with_terminal_io(
            Events(VecDeque::from([
                Ok(Event::MatchList(MatchListEvent::QueueMatches)),
                Ok(Event::Select),
            ])),
            &mut TestTerminal::default(),
        )
        .unwrap();
    assert!(selected.iter().copied().eq(["alpha", "beta", "gamma"]));
}

#[test]
fn preview_picker_uses_the_shared_loop_and_draws_a_pane() {
    let mut picker = picker(["alpha", "beta"]);
    let mut plain_terminal = TestTerminal::default();
    let selected = picker
        .pick_with_terminal_io(navigation(), &mut plain_terminal)
        .unwrap();
    assert_eq!(selected, Some(&"alpha"));

    let mut previewer = TestPreviewer::default();
    let mut terminal = TestTerminal::default();
    let mut preview_picker = picker.with_preview(&mut previewer);
    let selected = preview_picker
        .pick_with_terminal_io(navigation(), &mut terminal)
        .unwrap();
    assert_eq!(selected, Some(&"alpha"));
    assert_eq!(previewer.requested, ["alpha", "beta"]);
    assert!(
        !String::from_utf8(plain_terminal.output)
            .unwrap()
            .contains('╭')
    );
    assert!(String::from_utf8(terminal.output).unwrap().contains('╭'));
    assert_eq!(terminal.cleanups, 1);
}

#[test]
fn preview_picker_preserves_multiple_selection() {
    let picker = picker(["alpha", "beta"]);
    let mut preview_picker = picker.with_preview(TestPreviewer::default());
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        Ok(Event::Select),
    ]));
    let selected = preview_picker
        .as_multi()
        .pick_with_terminal_io(events, &mut TestTerminal::default())
        .unwrap();
    assert_eq!(
        selected.iter().copied().collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}

#[test]
fn picker_exits_cancel_all_pending_requests() {
    for (exit, succeeds) in [
        (Ok(Event::Select), true),
        (Ok(Event::Quit), true),
        (Ok(Event::QuitPromptEmpty), true),
        (Ok(Event::UserInterrupt), false),
        (Ok(Event::Abort("aborted")), false),
        (Ok(Event::Restart), false),
        (Err(RecvError::Disconnected), false),
        (Err(RecvError::IO(io::Error::other("read failed"))), false),
    ] {
        let picker = picker(["alpha", "beta"]);
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        let events = Events(VecDeque::from([
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            exit,
        ]));
        let mut terminal = TestTerminal::default();
        let mut preview_picker = picker.with_preview(&mut previewer);
        let result = preview_picker.pick_with_terminal_io(events, &mut terminal);

        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(terminal.cleanups, 1);
        assert_eq!(previewer.queued.len(), 2);
        assert!(
            previewer
                .queued
                .iter()
                .all(QueuedPreviewRequest::is_cancelled)
        );
        assert_eq!(previewer.focused, [Some("alpha"), Some("beta"), None]);
        assert_eq!(previewer.cancelled_on_blur, [true]);
    }
}

#[test]
fn terminal_errors_cancel_pending_requests() {
    for fail_write in [true, false] {
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
        let result = preview_picker
            .pick_with_terminal_io(Events(VecDeque::from([Ok(Event::Quit)])), &mut terminal);

        assert!(matches!(result, Err(PickError::IO(_))));
        assert_eq!(terminal.cleanups, 1);
        assert!(previewer.queued[0].is_cancelled());
        assert_eq!(previewer.focused, [Some("alpha"), None]);
        assert_eq!(previewer.cancelled_on_blur, [true]);
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
        let _ = preview_picker.pick_with_terminal_io(
            CallbackEvents(|| panic!("event source panic")),
            &mut terminal,
        );
    }));

    assert!(panic.is_err());
    assert_eq!(terminal.cleanups, 1);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.focused, [Some("alpha")]);
    assert!(previewer.cancelled_on_blur.is_empty());
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
                Events(VecDeque::from([Ok(Event::Quit)])),
                &mut TestTerminal::default(),
            )
            .unwrap();
    }
    assert_eq!(previewer.requested, ["alpha", "alpha", "alpha"]);
    assert_eq!(
        previewer
            .ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
}

#[test]
fn restart_invalidates_cached_item_ids() {
    struct RestartEvents {
        injectors: crate::event::Observer<crate::Injector<&'static str, StrRenderer>>,
        statuses: crate::event::Observer<crate::event::PickerStatus>,
        step: usize,
        started: Instant,
    }

    impl EventSource for RestartEvents {
        type AbortErr = &'static str;

        fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
            Err(RecvError::Timeout)
        }

        fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
            assert!(self.started.elapsed() < Duration::from_secs(5));
            match self.step {
                0 => {
                    self.step = 1;
                    Ok(Event::Restart)
                }
                1 => {
                    self.injectors.try_recv().unwrap().push("beta");
                    self.step = 2;
                    Ok(Event::Status { id: 0 })
                }
                2 => {
                    self.step = 3;
                    Err(RecvError::Timeout)
                }
                _ => {
                    if self.statuses.try_recv().unwrap().matched_item_count == 1 {
                        Ok(Event::Select)
                    } else {
                        self.step = 2;
                        Ok(Event::Status { id: 0 })
                    }
                }
            }
        }
    }

    let mut picker = picker(["alpha"]);
    let events = RestartEvents {
        injectors: picker.injector_observer(false),
        statuses: picker.status_observer(),
        step: 0,
        started: Instant::now(),
    };
    let mut previewer = TestPreviewer::default();
    let mut preview_picker = picker.with_preview(&mut previewer);
    let selected = preview_picker
        .pick_with_terminal_io(events, &mut TestTerminal::default())
        .unwrap();
    assert_eq!(selected, Some(&"beta"));
    assert_eq!(previewer.requested, ["alpha", "beta"]);
    assert_eq!(previewer.focused, [Some("alpha"), None, Some("beta"), None]);
    assert_ne!(previewer.ids[0], previewer.ids[1]);
    assert_eq!(previewer.restarts, 1);
}

#[test]
fn toggling_the_pane_reports_focus_and_coalesces_hidden_navigation() {
    for defer in [false, true] {
        for size in [(20, 10), (5, 10)] {
            let picker = picker(["alpha", "beta"]);
            let mut previewer = TestPreviewer {
                defer,
                ..TestPreviewer::default()
            };
            let events = Events(VecDeque::from([
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
                Ok(Event::Quit),
            ]));
            let mut terminal = TestTerminal {
                sizes: VecDeque::from([size]),
                ..TestTerminal::default()
            };
            picker
                .with_preview(&mut previewer)
                .pick_with_terminal_io(events, &mut terminal)
                .unwrap();
            assert_eq!(
                previewer.focused,
                [Some("alpha"), None, Some("beta"), None, Some("alpha"), None]
            );
            assert_eq!(
                previewer.requested,
                if defer {
                    &["alpha", "beta", "alpha"][..]
                } else {
                    &["alpha", "beta"][..]
                }
            );
        }
    }
}

#[test]
fn revealing_the_same_item_only_resubmits_queued_previews() {
    for defer in [false, true] {
        let picker = picker(["alpha", "beta"]);
        let mut previewer = TestPreviewer {
            defer,
            ..TestPreviewer::default()
        };
        let events = Events(VecDeque::from([
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Layout(LayoutEvent::TogglePreview)),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .with_preview(&mut previewer)
            .pick_with_terminal_io(events, &mut TestTerminal::default())
            .unwrap();
        assert_eq!(
            previewer.focused,
            [Some("alpha"), None, Some("alpha"), None]
        );
        assert_eq!(
            previewer.requested,
            if defer {
                &["alpha", "alpha"][..]
            } else {
                &["alpha"][..]
            }
        );
    }
}

#[test]
fn exiting_with_no_focus_does_not_notify_again() {
    for items in [&[][..], &["alpha"][..]] {
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        picker(items.iter().copied())
            .with_preview(&mut previewer)
            .pick_with_terminal_io(
                Events(VecDeque::from([
                    Ok(Event::Layout(LayoutEvent::TogglePreview)),
                    Err(RecvError::Timeout),
                    Ok(Event::Quit),
                ])),
                &mut TestTerminal::default(),
            )
            .unwrap();
        if items.is_empty() {
            assert!(previewer.focused.is_empty());
        } else {
            assert_eq!(previewer.focused, [Some("alpha"), None]);
            assert_eq!(previewer.cancelled_on_blur, [false]);
            assert!(previewer.queued[0].is_cancelled());
        }
    }
}
