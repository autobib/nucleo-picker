use std::{
    cell::Cell,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use super::{TestEvents, TestTerminal, frame_events};
use crate::{
    Picker, PickerOptions,
    event::{Event, LayoutEvent, MatchListEvent, PromptEvent, RecvError},
    interface::{Interface, Single, UncachedPreview},
    preview::{Pane, PreviewEvent},
    render::StrRenderer,
    session::{Components, Context, Exit},
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
fn startup_updates_matching_before_init_and_the_first_frame() {
    struct Startup<'a>(&'a Cell<bool>);

    impl Interface<&'static str> for Startup<'_> {
        type State = ();
        type Output<'items> = Option<&'items &'static str>;

        fn init(&mut self, cx: &Context<'_, &'static str>) {
            assert_eq!(cx.matches().len(), 1);
            assert_eq!(cx.hovered().unwrap().value(), &"beta");
            self.0.set(true);
        }

        fn before_frame(
            &mut self,
            _: &mut (),
            cx: &Context<'_, &'static str>,
            changed: bool,
            _: Instant,
        ) -> Components {
            assert!(self.0.get());
            assert!(changed);
            assert_eq!(cx.hovered().unwrap().value(), &"beta");
            Components::NONE
        }

        fn finish<'items>(
            &mut self,
            (): (),
            exit: Exit,
            cx: Context<'items, &'static str>,
        ) -> Self::Output<'items> {
            Single.finish((), exit, cx)
        }
    }

    for borrowed in [false, true] {
        let initialized = Cell::new(false);
        let mut picker = PickerOptions::new()
            .threads(std::num::NonZero::new(1))
            .frame_interval(Duration::from_secs(5))
            .query("^beta$")
            .picker(StrRenderer)
            .with_interface(Startup(&initialized));
        picker.push_batch(["alpha", "beta"]);
        let events = TestEvents(|_| Ok(Event::Select));
        let mut terminal = TestTerminal::default();
        let selected = if borrowed {
            picker
                .as_interface(Startup(&initialized))
                .pick_with_terminal_io(events, &mut terminal)
        } else {
            picker.pick_with_terminal_io(events, &mut terminal)
        }
        .unwrap();
        assert_eq!(selected, Some(&"beta"));
        assert!(initialized.get());
        assert!(String::from_utf8(terminal.output).unwrap().contains("beta"));
    }
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
fn host_only_repaints_render_and_report_changes() {
    for (event, size, clears) in [
        (Event::Redraw, (20, 8), 2),
        (Event::Redraw, (0, 8), 0),
        (Event::Layout(LayoutEvent::MoveDividerLeft(1)), (20, 8), 2),
    ] {
        let mut picker = PickerOptions::new()
            .picker(StrRenderer)
            .with_interface(UncachedPreview::new_single(|_: &&str, _: &mut Pane| {}));
        let statuses = picker.status_observer();
        let mut terminal = TestTerminal {
            size: Some(size),
            ..TestTerminal::default()
        };
        picker
            .pick_with_terminal_io(
                frame_events([
                    Ok(Event::Status { id: 1 }),
                    Ok(event),
                    Err(RecvError::Timeout),
                    Ok(Event::Quit),
                ]),
                &mut terminal,
            )
            .unwrap();
        let status = statuses.try_recv().unwrap();
        assert!(status.frame_changed);
        assert!(status.changed);
        assert_eq!(terminal.frame_changes.first(), Some(&true));
        assert_eq!(
            terminal
                .output
                .windows(4)
                .filter(|bytes| *bytes == b"\x1b[2J")
                .count(),
            clears,
        );
    }
}

#[test]
fn preview_events_follow_visibility_changes_within_the_batch() {
    for visible in [false, true] {
        let generated = Cell::new(0);
        let mut picker = PickerOptions::new()
            .frame_interval(Duration::from_secs(5))
            .picker(StrRenderer)
            .with_interface(UncachedPreview::new_single(|_: &&str, _: &mut Pane| {
                generated.set(generated.get() + 1);
            }));
        picker.push_batch(["alpha"]);
        picker.filter().unwrap();
        picker
            .pick_with_terminal_io(
                frame_events([
                    Ok(Event::Layout(LayoutEvent::TogglePreview)),
                    Ok(if visible {
                        Event::Layout(LayoutEvent::TogglePreview)
                    } else {
                        Event::Preview(PreviewEvent::Refresh)
                    }),
                    Ok(if visible {
                        Event::Preview(PreviewEvent::Refresh)
                    } else {
                        Event::Layout(LayoutEvent::TogglePreview)
                    }),
                    Err(RecvError::Timeout),
                    Ok(Event::Quit),
                ]),
                &mut TestTerminal::default(),
            )
            .unwrap();
        assert_eq!(generated.get(), 1 + usize::from(visible));
    }
}

#[test]
fn observers_survive_picker_and_view_conversions() {
    let mut picker = PickerOptions::new()
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    let old_injectors = picker.injector_observer(false);
    let injectors = picker.injector_observer(true);
    let old_statuses = picker.status_observer();
    let statuses = picker.status_observer();
    assert_eq!(
        old_injectors.try_recv().err(),
        Some(mpsc::TryRecvError::Disconnected),
    );
    assert_eq!(
        old_statuses.try_recv().err(),
        Some(mpsc::TryRecvError::Disconnected),
    );
    injectors.try_recv().unwrap().push_batch(["initial"]);
    assert_eq!(picker.filter().unwrap().len(), 1);

    let picker = picker.multi().single().with_interface(Single);
    #[cfg(feature = "preview-cache")]
    let picker = {
        let (picker, previewer) = picker.with_preview(()).multi().single().into_parts();
        picker.with_preview(previewer).without_preview()
    };
    let mut picker = picker;
    for (id, borrowed) in [(0, true), (1, false)] {
        let events = frame_events([
            Ok(Event::Restart),
            Ok(Event::Status { id }),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        let mut terminal = TestTerminal::default();
        if borrowed {
            picker
                .as_multi()
                .with_interface(Single)
                .pick_with_terminal_io(events, &mut terminal)
                .unwrap();
        } else {
            picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        }
        let status = statuses.try_recv().unwrap();
        assert_eq!(status.id, id);
        assert_eq!(status.item_count, 0);
        assert!(status.injecting);
        injectors.try_recv().unwrap().push_batch(["replacement"]);
        assert_eq!(
            picker.filter().unwrap().get(0).unwrap().value(),
            &"replacement",
        );
    }

    drop(picker);
    assert_eq!(
        injectors.try_recv().err(),
        Some(mpsc::TryRecvError::Disconnected),
    );
    assert_eq!(
        statuses.try_recv().err(),
        Some(mpsc::TryRecvError::Disconnected),
    );
}

#[test]
fn picker_layout_survives_interface_conversions() {
    fn preview(_: &&str, _: &mut Pane) {}

    for reversed in [None, Some(false), Some(true)] {
        for split in [None, Some(0.3)] {
            for borrowed in [false, true] {
                let mut options = PickerOptions::new().sort_results(false);
                if let Some(reversed) = reversed {
                    options = options.reversed(reversed);
                }
                if let Some(ratio) = split {
                    options = options.preview_split(ratio);
                }
                let picker = options
                    .query("")
                    .picker(StrRenderer)
                    .multi()
                    .single()
                    .with_interface(Single);
                #[cfg(feature = "preview-cache")]
                let picker = {
                    let (picker, previewer) = picker.with_preview(()).multi().single().into_parts();
                    picker.with_preview(previewer).without_preview()
                };
                let mut picker = picker.with_interface(UncachedPreview::new_single(preview));
                picker.push_batch(["alpha", "beta"]);
                picker.filter().unwrap();
                let reversed = reversed.unwrap_or(false);
                let navigate = if reversed {
                    MatchListEvent::Down(1)
                } else {
                    MatchListEvent::Up(1)
                };
                let mut events = [Event::MatchList(navigate), Event::Select].into_iter();
                let events = TestEvents(|_| Ok(events.next().unwrap()));
                let mut terminal = TestTerminal::default();
                let selected = if borrowed {
                    picker
                        .as_interface(Single)
                        .with_interface(UncachedPreview::new_single(preview))
                        .pick_with_terminal_io(events, &mut terminal)
                } else {
                    picker.pick_with_terminal_io(events, &mut terminal)
                }
                .unwrap();
                assert_eq!(selected, Some(&"beta"));
                let output = String::from_utf8(terminal.output).unwrap();
                assert!(output.ends_with(if reversed {
                    "\x1b[1;3H\x1b[?2026l"
                } else {
                    "\x1b[8;3H\x1b[?2026l"
                }));
                assert!(output.contains(if split.is_some() {
                    "\x1b[1;15H"
                } else {
                    "\x1b[1;11H"
                }));
            }
        }
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
        .picker(StrRenderer)
        .multi()
        .single();
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
        .as_multi()
        .with_interface(Single)
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
        for borrowed in [false, true] {
            let events = frame_events(
                redraw
                    .then_some(Ok(Event::Redraw))
                    .into_iter()
                    .chain((0..frames).map(|_| Err(RecvError::Timeout)))
                    .chain([Ok(Event::Quit)]),
            );
            let mut terminal = TestTerminal::default();
            let mut picker = PickerOptions::new()
                .frame_interval(Duration::from_millis(10))
                .background_frame_interval(Duration::from_millis(25))
                .query("")
                .picker::<&str, _>(StrRenderer)
                .multi()
                .single()
                .with_interface(Single);
            let _injector = injecting.then(|| picker.injector());
            if borrowed {
                picker
                    .as_multi()
                    .with_interface(Single)
                    .pick_with_terminal_io(events, &mut terminal)
                    .unwrap();
            } else {
                picker.pick_with_terminal_io(events, &mut terminal).unwrap();
            }
            assert_eq!(terminal.size_reads, calls, "borrowed={borrowed}");
        }
    }
}
