use std::{
    convert::Infallible,
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::{
    Picker, PickerOptions, Terminal,
    component::NoPreview,
    event::{Event, EventSource, PromptEvent, RecvError},
    render::StrRenderer,
};

#[derive(Default)]
struct TestTerminal {
    output: Vec<u8>,
    initial_render_delay: Duration,
    initial_render_end: Option<Instant>,
    first_frame: Option<Instant>,
}

impl io::Write for TestTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
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
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        Ok((20, 8))
    }

    fn end_render(&mut self) -> io::Result<()> {
        let delay = std::mem::take(&mut self.initial_render_delay);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        self.initial_render_end.get_or_insert_with(Instant::now);
        Ok(())
    }

    fn end_frame(&mut self, _changed: bool) -> io::Result<()> {
        self.first_frame.get_or_insert_with(Instant::now);
        Ok(())
    }
}

struct TestEvents<F>(F);

impl<F: FnMut(Duration) -> Result<Event, RecvError>> EventSource for TestEvents<F> {
    type AbortErr = Infallible;

    fn recv_timeout(&mut self, duration: Duration) -> Result<Event, RecvError> {
        (self.0)(duration)
    }
}

fn pick(
    picker: &mut Picker<&'static str, StrRenderer>,
    events: impl IntoIterator<Item = Event>,
) -> Option<&'static str> {
    let (sender, receiver) = mpsc::channel();
    for event in events {
        sender.send(event).unwrap();
    }
    drop(sender);
    picker
        .pick_impl::<_, _, (), _>(receiver, &mut TestTerminal::default(), NoPreview::new())
        .unwrap()
        .copied()
}

#[test]
fn initial_query_matches_the_normalized_prompt() {
    let mut picker = PickerOptions::new().query("a\u{7}b").picker(StrRenderer);
    picker.extend(["ab", "cd"]);
    while picker.engine.update(5).matching {}

    assert_eq!(picker.query(), "ab");
    assert_eq!(picker.engine.snapshot().matched_item_count(), 1);
    assert_eq!(pick(&mut picker, [Event::Select]), Some("ab"));

    pick(
        &mut picker,
        [
            Event::Prompt(PromptEvent::Reset("ab".to_owned())),
            Event::Quit,
        ],
    );
    while picker.engine.update(5).matching {}

    assert_eq!(picker.query(), "ab");
    assert_eq!(picker.engine.snapshot().matched_item_count(), 1);
    assert_eq!(pick(&mut picker, [Event::Select]), Some("ab"));
}

#[test]
fn quit_preserves_the_buffered_prompt_event() {
    let mut picker = Picker::new(StrRenderer);

    assert_eq!(
        pick(
            &mut picker,
            [Event::Prompt(PromptEvent::Insert('x')), Event::Quit],
        ),
        None,
    );
    assert_eq!(picker.query(), "x");
}

#[test]
fn select_preserves_the_buffered_prompt_event_and_current_selection() {
    let mut picker = Picker::new(StrRenderer);
    picker.extend(["alpha"]);
    while picker.engine.update(5).matching {}

    assert_eq!(
        pick(
            &mut picker,
            [Event::Prompt(PromptEvent::Insert('x')), Event::Select],
        ),
        Some("alpha"),
    );
    assert_eq!(picker.query(), "x");
}

#[test]
fn reused_picker_preserves_navigation_on_exit() {
    for reversed in [false, true] {
        for (exit, expected) in [(Event::Select, Some("beta")), (Event::Quit, None)] {
            let mut picker = PickerOptions::new()
                .sort_results(false)
                .reversed(reversed)
                .frame_interval(Duration::from_secs(1))
                .picker(StrRenderer);
            picker.push_batch(["alpha", "beta"]);
            picker.filter().unwrap();

            let navigate = if reversed {
                crate::event::MatchListEvent::Down(1)
            } else {
                crate::event::MatchListEvent::Up(1)
            };
            assert_eq!(
                pick(&mut picker, [Event::MatchList(navigate), exit]),
                expected,
            );
            assert_eq!(pick(&mut picker, [Event::Select]), Some("beta"));
        }
    }
}

#[test]
fn reused_picker_matches_the_query_saved_on_exit() {
    let mut picker = Picker::new(StrRenderer);
    picker.extend(["alpha", "beta"]);
    while picker.engine.update(5).matching {}

    pick(
        &mut picker,
        [
            Event::Prompt(PromptEvent::Reset("beta".to_owned())),
            Event::Quit,
        ],
    );
    let status = picker.status_observer();
    let (sender, receiver) = mpsc::channel::<Event>();
    let input = thread::spawn(move || {
        for id in 0.. {
            sender.send(Event::Status { id }).unwrap();
            if !status
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .matching
            {
                break;
            }
        }
        sender.send(Event::Select).unwrap();
    });
    let selected = picker
        .pick_impl::<_, _, (), _>(receiver, &mut TestTerminal::default(), NoPreview::new())
        .unwrap()
        .copied();
    input.join().unwrap();

    assert_eq!(selected, Some("beta"));
    assert_eq!(picker.query(), "beta");
}

#[test]
fn busy_events_do_not_starve_frames_or_status() {
    for interval in [
        Duration::ZERO,
        Duration::from_micros(100),
        Duration::from_millis(15),
    ] {
        let mut picker: Picker<&str, _> = PickerOptions::new()
            .frame_interval(interval)
            .picker(StrRenderer);
        let status = picker.status_observer();
        let mut received = 0;
        let events = TestEvents(|_| {
            received += 1;
            Ok(match received {
                1 => Event::Prompt(PromptEvent::Insert('x')),
                2 => Event::Status { id: 7 },
                3..=10 => {
                    thread::sleep(Duration::from_millis(5));
                    Event::Redraw
                }
                _ => Event::Quit,
            })
        });
        let mut terminal = TestTerminal::default();

        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, NoPreview::new())
            .unwrap();

        assert!(terminal.first_frame.is_some());
        let response = status.try_recv().unwrap();
        assert_eq!(response.id, 7);
        assert_eq!(response.query, "x");
        assert!(response.changed);
    }
}

#[test]
fn slow_render_leaves_time_to_drain() {
    let mut picker: Picker<&str, _> = PickerOptions::new()
        .frame_interval(Duration::from_millis(1))
        .picker(StrRenderer);
    let mut received = 0;
    let events = TestEvents(|_| {
        received += 1;
        Ok(match received {
            1 => Event::Redraw,
            2..=4 => {
                thread::sleep(Duration::from_millis(2));
                Event::Redraw
            }
            _ => Event::Quit,
        })
    });
    let mut terminal = TestTerminal {
        initial_render_delay: Duration::from_millis(5),
        ..TestTerminal::default()
    };

    picker
        .pick_impl::<_, _, (), _>(events, &mut terminal, NoPreview::new())
        .unwrap();

    assert!(
        terminal.first_frame.unwrap()
            >= terminal.initial_render_end.unwrap() + Duration::from_millis(1)
    );
}

#[test]
fn drain_reserve_does_not_wait_for_more_events() {
    let mut picker: Picker<&str, _> = PickerOptions::new()
        .frame_interval(Duration::ZERO)
        .picker(StrRenderer);
    let status = picker.status_observer();
    let mut received = 0;
    let events = TestEvents(|duration: Duration| {
        assert!(duration.is_zero());
        received += 1;
        match received {
            1 => Ok(Event::Status { id: 9 }),
            2 => Err(RecvError::Timeout),
            _ => Ok(Event::Quit),
        }
    });

    picker
        .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), NoPreview::new())
        .unwrap();

    assert_eq!(status.try_recv().unwrap().id, 9);
}

#[test]
fn headless_updates_rebuild_layout_when_reentering_the_picker() {
    fn draw(picker: &mut Picker<&'static str, StrRenderer>) -> Vec<u8> {
        let mut terminal = TestTerminal::default();
        picker
            .pick_impl::<_, _, (), _>(
                TestEvents(|_| Ok(Event::Quit)),
                &mut terminal,
                NoPreview::new(),
            )
            .unwrap();
        terminal.output
    }

    for reversed in [false, true] {
        let items = ["alpha\nsecond line", "beta", "gamma\nsecond\nthird"];
        let options = || {
            PickerOptions::new()
                .threads(std::num::NonZero::new(1))
                .sort_results(false)
                .reversed(reversed)
        };
        let mut picker = options().picker(StrRenderer);
        picker.push_batch(items);
        picker.filter().unwrap();
        draw(&mut picker);

        for query in ["^beta$", "^alpha", "absent", ""] {
            picker.update_query(query);
            picker.filter().unwrap();
            let mut fresh = options().query(query).picker(StrRenderer);
            fresh.push_batch(items);
            fresh.filter().unwrap();
            assert_eq!(draw(&mut picker), draw(&mut fresh), "query: {query:?}");
        }

        let navigate = if reversed {
            crate::event::MatchListEvent::Down(2)
        } else {
            crate::event::MatchListEvent::Up(2)
        };
        assert_eq!(
            pick(&mut picker, [Event::MatchList(navigate), Event::Select]),
            Some(items[2]),
        );
        picker.update_query("^beta$");
        picker.filter().unwrap();
        assert_eq!(pick(&mut picker, [Event::Select]), Some("beta"));
    }
}

#[test]
fn open_input_can_fall_back_to_interactive_picking() {
    let mut picker = Picker::new(StrRenderer);
    let injector = picker.injector();
    injector.push("alpha");
    let start = Instant::now();
    while picker.update_matches(Duration::from_millis(5)).matching {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    }

    let count = picker.filter().map(|matches| matches.len());
    assert_eq!(count, None);
    let selected = match count {
        Some(0) => None,
        Some(1) => picker.matches().get(0).copied(),
        _ => pick(&mut picker, [Event::Select]),
    };
    assert_eq!(selected, Some("alpha"));

    injector.push("beta");
    drop(injector);
    assert_eq!(picker.filter().unwrap().len(), 2);
}

#[cfg(all(feature = "preview", feature = "unstable-backend"))]
mod preview;
