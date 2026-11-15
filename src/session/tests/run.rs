use std::{sync::mpsc, thread, time::Duration};

use super::{TestEvents, TestTerminal, frame_events};
use crate::{
    Picker, PickerOptions,
    event::{Event, MatchListEvent, PromptEvent, RecvError},
    render::StrRenderer,
};

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
        .pick_with_terminal_io(receiver, &mut TestTerminal::default())
        .unwrap()
        .copied()
}

#[test]
fn exits_commit_buffered_edits_without_rematching_the_selection() {
    for (exit, expected) in [(Event::Select, Some("beta")), (Event::Quit, None)] {
        let mut picker = PickerOptions::new().sort_results(false).picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        picker.filter().unwrap();
        assert_eq!(
            pick(
                &mut picker,
                [
                    Event::MatchList(MatchListEvent::Up(1)),
                    Event::Prompt(PromptEvent::Reset("^alpha$".to_owned())),
                    exit,
                ]
            ),
            expected,
        );
        assert_eq!(picker.query(), "^alpha$");
        picker.filter().unwrap();
        assert_eq!(pick(&mut picker, [Event::Select]), Some("alpha"));
    }
}

#[test]
fn quit_prompt_empty_flushes_edits_before_deciding_to_exit() {
    let mut picker = Picker::new(StrRenderer);
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    assert_eq!(
        pick(
            &mut picker,
            [
                Event::Prompt(PromptEvent::Insert('x')),
                Event::QuitPromptEmpty,
                Event::Prompt(PromptEvent::Backspace(1)),
                Event::QuitPromptEmpty,
                Event::Select,
            ]
        ),
        None
    );
    assert_eq!(picker.query(), "");
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

        picker.pick_with_terminal_io(events, &mut terminal).unwrap();

        assert!(terminal.first_frame.is_some());
        let response = status.try_recv().unwrap();
        assert_eq!(response.id, 7);
        assert_eq!(response.query, "x");
        assert!(response.changed);
        assert!(response.frame_changed);
    }
}

#[test]
fn status_changes_accumulate_and_reset_when_responses_are_produced() {
    for read_responses in [false, true] {
        let mut picker: Picker<&str, _> = PickerOptions::new()
            .frame_interval(Duration::ZERO)
            .picker(StrRenderer);
        let statuses = picker.status_observer();
        let mut responses = Vec::new();
        let mut script = [
            Ok(Event::Status { id: 0 }),
            Err(RecvError::Timeout),
            Ok(Event::Redraw),
            Err(RecvError::Timeout),
            Err(RecvError::Timeout),
            Ok(Event::Status { id: 1 }),
            Err(RecvError::Timeout),
            Ok(Event::Status { id: 2 }),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]
        .into_iter();
        let events = TestEvents(|_| {
            if read_responses && let Ok(status) = statuses.try_recv() {
                responses.push((status.id, status.changed, status.frame_changed));
            }
            script.next().unwrap()
        });
        picker
            .pick_with_terminal_io(events, &mut TestTerminal::default())
            .unwrap();
        if let Ok(status) = statuses.try_recv() {
            responses.push((status.id, status.changed, status.frame_changed));
        }
        let expected = if read_responses {
            &[(0, true, false), (1, true, false), (2, false, false)][..]
        } else {
            &[(2, false, false)]
        };
        assert_eq!(responses, expected, "read_responses={read_responses}");
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

    picker.pick_with_terminal_io(events, &mut terminal).unwrap();

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
        .pick_with_terminal_io(events, &mut TestTerminal::default())
        .unwrap();

    assert_eq!(status.try_recv().unwrap().id, 9);
}

#[test]
fn headless_updates_rebuild_layout_when_reentering_the_picker() {
    fn draw(picker: &mut Picker<&'static str, StrRenderer>) -> Vec<u8> {
        let mut terminal = TestTerminal::default();
        picker
            .pick_with_terminal_io(TestEvents(|_| Ok(Event::Quit)), &mut terminal)
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
            MatchListEvent::Down(2)
        } else {
            MatchListEvent::Up(2)
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
fn size_queries_follow_redraws_and_background_frequency() {
    for (redraw, frames, injecting, calls) in [
        (false, 1, false, 1),
        (true, 1, false, 2),
        (false, 7, true, 3),
    ] {
        let events = frame_events(
            redraw
                .then_some(Ok(Event::Redraw))
                .into_iter()
                .chain((0..frames).map(|_| Err(RecvError::Timeout)))
                .chain([Ok(Event::Quit)]),
        );
        let mut terminal = TestTerminal::default();
        let mut picker = PickerOptions::new()
            .frame_interval(Duration::from_millis(1))
            .background_frame_interval(Duration::from_millis(3))
            .picker::<&str, _>(StrRenderer);
        let _injector = injecting.then(|| picker.injector());
        picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        assert_eq!(terminal.size_reads, calls);
    }
}
