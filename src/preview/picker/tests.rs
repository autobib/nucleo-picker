use super::*;
use crate::{PickerOptions, preview::QueuedPreviewRequest, render::StrRenderer};

#[derive(Default)]
struct TestPreviewer {
    requested: Vec<&'static str>,
    queued: Vec<QueuedPreviewRequest>,
    defer: bool,
    fail: bool,
}

impl Preview<&'static str> for TestPreviewer {
    type AbortErr = &'static str;

    fn preview(
        &mut self,
        item: &&'static str,
        request: PreviewRequest,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        assert!(timeout >= Duration::from_millis(2));
        self.requested.push(item);
        if self.fail {
            return Err("preview failed");
        }
        if self.defer {
            let (pending, queued) = request.defer();
            self.queued.push(queued);
            Ok(PreviewResponse::Pending(pending))
        } else {
            let mut buffer = request.ready();
            buffer.push_str(item);
            Ok(PreviewResponse::Ready(buffer))
        }
    }
}

fn settle(picker: &mut Picker<&'static str, StrRenderer>) {
    let start = std::time::Instant::now();
    while picker.match_list.update(5).matching {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}

fn picker(items: impl IntoIterator<Item = &'static str>) -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    settle(&mut picker);
    picker.match_list.resize(8);
    picker
}

#[test]
fn cache_tracks_item_identity_and_preserves_scroll_state() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewSession::new(TestPreviewer::default());
    session.update(&picker.match_list, Instant::now()).unwrap();
    let alpha = picker.match_list.selected_item().unwrap().0;
    session.cache.get_mut(&alpha).unwrap().scroll_position = 7;

    picker.update_query("beta");
    settle(&mut picker);
    assert_eq!(picker.match_list.selection(), Some(0));
    session.update(&picker.match_list, Instant::now()).unwrap();
    let beta = picker.match_list.selected_item().unwrap().0;
    assert_ne!(alpha, beta);

    picker.update_query("");
    settle(&mut picker);
    picker.match_list.set_selection(0);
    session.update(&picker.match_list, Instant::now()).unwrap();
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    let cached = session.cache.peek(&alpha).unwrap();
    assert_eq!(cached.scroll_position, 7);
    let Some(State::Ready(buffer)) = &cached.state else {
        panic!("expected cached ready preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
}

#[test]
fn cache_evicts_the_least_recently_visited_item() {
    let mut picker = picker(std::iter::repeat_n("item", 65));
    let mut session = PreviewSession::new(TestPreviewer::default());
    for selection in 0..64 {
        picker.match_list.set_selection(selection);
        session.update(&picker.match_list, Instant::now()).unwrap();
    }
    picker.match_list.set_selection(0);
    session.update(&picker.match_list, Instant::now()).unwrap();
    picker.match_list.set_selection(64);
    session.update(&picker.match_list, Instant::now()).unwrap();

    assert_eq!(session.cache.len(), 64);
    assert!(session.cache.contains(&0));
    assert!(!session.cache.contains(&1));
    assert_eq!(session.previewer.requested.len(), 65);

    picker.match_list.set_selection(1);
    session.update(&picker.match_list, Instant::now()).unwrap();
    assert_eq!(session.previewer.requested.len(), 66);
    assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 0);
}

#[test]
fn pending_previews_are_cached_without_duplicate_requests() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewSession::new(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    for selection in [0, 0, 1, 0] {
        picker.match_list.set_selection(selection);
        session.update(&picker.match_list, Instant::now()).unwrap();
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert!(matches!(
        session.cache.peek(&0).unwrap().state,
        Some(State::Pending(_))
    ));
}

#[test]
fn an_empty_match_list_does_not_request_previews() {
    let picker = picker([]);
    let mut session = PreviewSession::new(TestPreviewer::default());
    session.update(&picker.match_list, Instant::now()).unwrap();
    assert!(session.previewer.requested.is_empty());
    assert!(session.cache.is_empty());
}

#[cfg(feature = "unstable-backend")]
mod picker_loop {
    use std::collections::VecDeque;

    use super::*;
    use crate::{
        Terminal,
        event::{MatchListEvent, RecvError},
    };

    struct Events(VecDeque<Result<Event<&'static str>, RecvError>>);

    impl EventSource for Events {
        type AbortErr = &'static str;

        fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
            self.0.pop_front().expect("event script exhausted")
        }
    }

    #[derive(Default)]
    struct TestTerminal {
        output: Vec<u8>,
        cleanups: usize,
    }

    impl io::Write for TestTerminal {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
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
            Ok(())
        }

        fn size(&mut self) -> io::Result<(u16, u16)> {
            Ok((20, 10))
        }
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
    fn preview_picker_uses_the_shared_loop_and_leaves_output_unchanged() {
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
        assert_eq!(terminal.output, plain_terminal.output);
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
    }
}
