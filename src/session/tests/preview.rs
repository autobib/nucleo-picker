use std::{cell::Cell, collections::VecDeque, rc::Rc};

use crate::{
    PickerOptions,
    component::{Component, PreviewComponent},
    error::PickError,
    event::{Event, EventSource, LayoutEvent},
    preview::{
        PreviewBuffer, PreviewConfig, PreviewEvent, SyncPreviewer,
        pane::{PreviewPane, tests::support::*},
        request::QueuedPreviewRequest,
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
fn interleaved_navigation_and_scrolling_target_the_buffered_item() {
    for reversed in [false, true] {
        let mut picker = PickerOptions::new()
            .reverse_items(false)
            .reversed(reversed)
            .picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        settle(&mut picker);
        picker
            .list_state
            .layout
            .resize(picker.engine.snapshot(), 8, &picker.list_state.config);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            1,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            0,
            &picker.list_state.config,
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
            .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
            .unwrap();
        assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 3);
        assert_eq!(session.test_cache().peek(&1).unwrap().scroll_position, 7);
        assert_eq!(session.test_previewer().requested, ["beta", "alpha"]);
    }
}

#[test]
fn batched_navigation_and_refresh_target_the_event_selection() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    session
        .update(Some(1), &picker.engine, Instant::now(), true)
        .unwrap();
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
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(
        session.test_previewer().requested,
        ["beta", "alpha", "beta"]
    );
    assert_eq!(terminal.changed, [false, true]);
}

#[test]
fn refresh_errors_abort_and_clean_up_the_terminal() {
    for defer in [false, true] {
        let mut picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer,
            fail_after: Some(1),
            ..TestPreviewer::default()
        };
        let mut terminal = TestTerminal::default();
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Refresh)),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        let mut preview_picker = picker.with_preview(&mut previewer);
        let result = preview_picker.pick_with_terminal_io(events, &mut terminal);
        assert!(matches!(result, Err(PickError::Aborted("preview failed"))));
        assert_eq!(terminal.cleanups, 1);
        assert_eq!(previewer.requested, ["alpha", "alpha"]);
        assert!(
            previewer
                .queued
                .iter()
                .all(QueuedPreviewRequest::is_cancelled)
        );
    }
}

#[test]
fn refresh_does_not_run_the_previewer_before_a_following_quit() {
    for defer in [false, true] {
        let mut picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer,
            fail_after: Some(1),
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
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&1).unwrap().scroll_position, 8);
    assert!(!session.test_cache().contains(&0));
}

#[test]
fn scrolling_a_missing_preview_does_not_request_it() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 0);
    assert!(!session.test_cache().contains(&1));
}

#[test]
fn scrolling_pending_previews_is_ignored_by_the_loop() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    let mut terminal = TestTerminal::default();
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 0);
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(terminal.changed, [false]);
}

#[test]
fn idle_frames_collect_completed_previews_and_enable_scrolling() {
    for size in [(20, 10), (30, 12)] {
        let mut picker = picker(["alpha"]);
        let status = picker.status_observer();
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
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
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        let status = status.try_recv().unwrap();
        assert_eq!((status.width, status.height), size);
        assert_eq!(terminal.changed, [false, true, true, false]);
        assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 1);
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
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::Up(1))),
        Ok(Event::MatchList(MatchListEvent::Down(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    let mut terminal = TestTerminal::default();
    picker
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(terminal.changed, [false]);
    assert_eq!(session.test_previewer().requested, ["alpha"]);
    assert_eq!(session.test_previewer().focused, [Some("alpha")]);
    assert!(!session.test_previewer().queued[0].is_cancelled());
}

#[test]
fn retry_and_promotion_errors_abort_and_clean_up_the_terminal() {
    for retry in [false, true] {
        let mut picker = picker(["alpha", "beta"]);
        let mut previewer = TestPreviewer {
            defer: true,
            drop_requests: retry,
            fail_after: Some(if retry { 1 } else { 2 }),
            ..TestPreviewer::default()
        };
        let mut terminal = TestTerminal::default();
        let events = if retry {
            VecDeque::from([Err(RecvError::Timeout)])
        } else {
            VecDeque::from([
                Ok(Event::MatchList(MatchListEvent::Up(1))),
                Err(RecvError::Timeout),
                Ok(Event::MatchList(MatchListEvent::Down(1))),
                Err(RecvError::Timeout),
            ])
        };
        let mut preview_picker = picker.with_preview(&mut previewer);
        let result = preview_picker.pick_with_terminal_io(Events(events), &mut terminal);
        assert!(matches!(result, Err(PickError::Aborted("preview failed"))));
        assert_eq!(terminal.cleanups, 1);
        if retry {
            assert_eq!(previewer.requested, ["alpha", "alpha"]);
        } else {
            assert_eq!(previewer.requested, ["alpha", "beta", "alpha"]);
            assert!(previewer.queued[0].is_cancelled());
            assert!(previewer.queued[1].is_cancelled());
        }
    }
}

#[test]
fn scroll_changes_are_reported_without_redrawing_the_match_list() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    let mut terminal = TestTerminal::default();
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::PageDown(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 8);
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
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    session.test_previewer().focused.clear();
    session.test_cache().get_mut(&0).unwrap().scroll_position = 25;
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
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 22);
    assert_eq!(session.test_previewer().focused, [Some("alpha")]);
    assert_eq!(session.test_previewer().requested, ["alpha"]);
}

#[test]
fn resizing_reclamps_the_offset_and_changes_page_height() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
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
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 6);
}

#[test]
fn initial_layout_resizes_retained_preview_state() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    session.resize(
        Area {
            width: 22,
            height: 6,
            ..Area::default()
        },
        &picker.engine,
    );
    session.handle(PreviewEvent::Down(usize::MAX), Some(0));
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 26);

    picker
        .pick_impl::<_, _, (), _>(
            Events(VecDeque::from([Ok(Event::Quit)])),
            &mut TestTerminal::default(),
            &mut session,
        )
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 22);
}

#[test]
fn resizing_clamps_cached_previews_before_they_are_revisited() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    session.resize(
        Area {
            width: 22,
            height: 10,
            ..Area::default()
        },
        &picker.engine,
    );
    session.handle(PreviewEvent::Down(usize::MAX), Some(0));
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
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
        .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 18);
    assert_eq!(session.test_previewer().requested, ["alpha", "beta"]);
}

#[test]
fn scrolling_down_down_up_at_the_bottom_moves_up_one_line() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    let events = Events(VecDeque::from([
        Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::Down(1))),
        Ok(Event::Preview(PreviewEvent::Up(1))),
        Err(RecvError::Timeout),
        Ok(Event::Quit),
    ]));
    picker
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
        .unwrap();
    assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 21);
}

#[test]
fn zero_sized_panes_ignore_scrolling() {
    for size in [(0, 10), (5, 10), (20, 2), (20, 0)] {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
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
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.test_cache().peek(&0).unwrap().scroll_position, 0);
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
    let mut picker = picker(["alpha", "beta"]);
    let mut preview_picker = picker.with_preview(TestPreviewer::default());
    let events = Events(VecDeque::from([
        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        Err(RecvError::Timeout),
        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        Ok(Event::Select),
    ]));
    let selected = preview_picker
        .pick_multi_with_terminal_io(events, &mut TestTerminal::default())
        .unwrap();
    assert_eq!(
        selected.iter().copied().collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}

#[test]
fn preview_errors_abort_and_clean_up_the_terminal() {
    let mut picker = picker(["alpha"]);
    let mut preview_picker = picker.with_preview(TestPreviewer {
        fail: true,
        ..TestPreviewer::default()
    });
    let mut terminal = TestTerminal::default();
    let result = preview_picker.pick_with_terminal_io(Events(VecDeque::new()), &mut terminal);
    assert!(matches!(result, Err(PickError::Aborted("preview failed"))));
    assert_eq!(terminal.cleanups, 1);
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
        let mut picker = picker(["alpha", "beta"]);
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
    }
}

#[test]
fn terminal_errors_cancel_pending_requests() {
    for fail_write in [true, false] {
        let mut picker = picker(["alpha"]);
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
    }
}

#[test]
fn unwinding_cancels_pending_requests() {
    let mut picker = picker(["alpha"]);
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
}

#[test]
fn each_pick_session_starts_with_a_fresh_cache() {
    let mut picker = picker(["alpha"]);
    let mut previewer = TestPreviewer::default();
    let mut preview_picker = picker.with_preview(&mut previewer);
    for _ in 0..2 {
        preview_picker
            .pick_with_terminal_io(
                Events(VecDeque::from([Ok(Event::Quit)])),
                &mut TestTerminal::default(),
            )
            .unwrap();
    }
    assert_eq!(previewer.requested, ["alpha", "alpha"]);
    assert_eq!(previewer.focused, [Some("alpha"), Some("alpha")]);
    assert_eq!(previewer.ids[0], previewer.ids[1]);
}

#[test]
fn replacing_items_between_sessions_changes_ids() {
    let mut picker = picker(["alpha"]);
    let mut previewer = TestPreviewer::default();
    for step in 0..3 {
        match step {
            0 => {}
            1 => picker.restart(),
            2 => picker.reset_renderer(StrRenderer),
            _ => unreachable!(),
        }
        if step != 0 {
            picker.push_batch(["alpha"]);
            settle(&mut picker);
        }
        picker
            .with_preview(&mut previewer)
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
        injectors: crate::Observer<crate::Injector<&'static str, StrRenderer>>,
        statuses: crate::Observer<crate::event::PickerStatus>,
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
    assert_eq!(previewer.focused, [Some("alpha"), None, Some("beta")]);
    assert_ne!(previewer.ids[0], previewer.ids[1]);
}

#[test]
fn toggling_the_pane_reports_focus_and_coalesces_hidden_navigation() {
    for defer in [false, true] {
        for size in [(20, 10), (5, 10)] {
            let mut picker = picker(["alpha", "beta"]);
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
                [Some("alpha"), None, Some("beta"), None, Some("alpha")]
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
fn revealing_the_same_item_preserves_its_request_after_hidden_navigation() {
    for defer in [false, true] {
        let mut picker = picker(["alpha", "beta"]);
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
        assert_eq!(previewer.focused, [Some("alpha"), None, Some("alpha")]);
        assert_eq!(previewer.requested, ["alpha"]);
    }
}
