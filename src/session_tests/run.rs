use std::{
    cell::Cell,
    io,
    sync::mpsc,
    time::{Duration, Instant},
};

use super::{TestEvents, TestTerminal, frame_events};
use crate::{
    Picker, PickerOptions, Terminal,
    event::{Event, LayoutEvent, MatchListEvent, PreviewEvent, PromptEvent, RecvError},
    interface::{Interface, Single, UncachedPreview},
    preview::Pane,
    render::StrRenderer,
    session::{Components, Context, Dimensions, Exit},
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

    let initialized = Cell::new(false);
    let mut picker = PickerOptions::new()
        .threads(std::num::NonZero::new(1))
        .frame_interval(Duration::from_secs(5))
        .query("^beta$")
        .picker(StrRenderer)
        .with_interface(Startup(&initialized));
    picker.push_batch(["alpha", "beta"]);
    let mut terminal = TestTerminal::default();
    let selected = picker
        .pick_with_terminal_io(TestEvents(|_| Ok(Event::Select)), &mut terminal)
        .unwrap();
    assert_eq!(selected, Some(&"beta"));
    assert!(initialized.get());
    assert!(String::from_utf8(terminal.output).unwrap().contains("beta"));
}

#[test]
fn exits_commit_buffered_edits_without_rematching_the_selection() {
    for (exit, expected) in [(Event::Select, Some("beta")), (Event::Quit, None)] {
        let mut picker = PickerOptions::new().sort_results(false).picker(StrRenderer);
        picker.push_batch(["alpha", "beta"]);
        picker.filter().unwrap();
        let generation = picker.generation();
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
        assert_eq!(picker.generation(), generation);
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
    for (interval, initial_render_delay) in [
        (Duration::ZERO, Duration::ZERO),
        (Duration::from_micros(100), Duration::ZERO),
        (Duration::from_millis(15), Duration::ZERO),
        (Duration::from_millis(1), Duration::from_millis(5)),
    ] {
        let mut picker: Picker<&str, _> = PickerOptions::new()
            .frame_interval(interval)
            .picker(StrRenderer);
        let status = picker.status_observer();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut response = None;
        let mut received = 0;
        let events = TestEvents(|_| {
            assert!(Instant::now() < deadline, "busy events starved status");
            if let Ok(status) = status.try_recv() {
                response = Some(status);
                return Ok(Event::Quit);
            }
            received += 1;
            Ok(match received {
                1 => Event::Prompt(PromptEvent::Insert('x')),
                2 => Event::Status { id: 7 },
                _ => Event::Redraw,
            })
        });
        let mut terminal = TestTerminal {
            initial_render_delay,
            ..TestTerminal::default()
        };

        picker.pick_with_terminal_io(events, &mut terminal).unwrap();

        assert!(terminal.frame_changes.contains(&true));
        let response = response.unwrap();
        assert_eq!(response.id, 7);
        assert_eq!(response.generation, picker.generation());
        assert_eq!(response.query, "x");
        assert!(response.changed);
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
fn host_only_repaints_report_changes_even_without_writes() {
    for (event, size) in [
        (Event::Redraw, (20, 8)),
        (Event::Redraw, (0, 8)),
        (Event::Layout(LayoutEvent::MoveDividerLeft(1)), (20, 8)),
    ] {
        let mut picker = PickerOptions::new()
            .frame_interval(Duration::from_secs(5))
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
                    Ok(Event::Status { id: 0 }),
                    Err(RecvError::Timeout),
                    Ok(Event::Status { id: 1 }),
                    Ok(event),
                    Err(RecvError::Timeout),
                    Ok(Event::Quit),
                ]),
                &mut terminal,
            )
            .unwrap();
        let status = statuses.try_recv().unwrap();
        assert_eq!(status.id, 1);
        assert!(status.frame_changed);
        assert!(status.changed);
        assert_eq!(terminal.frame_changes.get(1), Some(&true));
        if size.0 == 0 {
            assert!(terminal.output.is_empty());
        }
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
    let initial_generation = picker.generation();
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
    {
        let injector = injectors.try_recv().unwrap();
        assert_eq!(injector.generation(), initial_generation);
        injector.push_batch(["initial"]);
    }
    assert_eq!(picker.filter().unwrap().len(), 1);

    let picker = picker.multi().single().with_interface(Single);
    #[cfg(feature = "preview-cache")]
    let picker = {
        let (picker, previewer) = picker.with_preview(()).multi().single().into_parts();
        picker.with_preview(previewer).without_preview()
    };
    let mut picker = picker;
    assert_eq!(picker.generation(), initial_generation);
    for (id, borrowed) in [(0, true), (1, false)] {
        let previous_generation = picker.generation();
        let events = frame_events([
            Ok(Event::Restart),
            Ok(Event::Status { id }),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]);
        let mut terminal = TestTerminal::default();
        if borrowed {
            let view = picker.as_multi().with_interface(Single);
            assert_eq!(view.generation(), previous_generation);
            view.pick_with_terminal_io(events, &mut terminal).unwrap();
        } else {
            picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        }
        let status = statuses.try_recv().unwrap();
        assert_eq!(status.id, id);
        assert_eq!(status.generation, picker.generation());
        assert_ne!(status.generation, previous_generation);
        assert_ne!(status.generation, initial_generation);
        assert_eq!(status.item_count, 0);
        assert!(status.injecting);
        {
            let injector = injectors.try_recv().unwrap();
            assert_eq!(injector.generation(), status.generation);
            injector.push_batch(["replacement"]);
        }
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

    for reversed in [false, true] {
        for split in [0.5, 0.3] {
            let options = || {
                PickerOptions::new()
                    .sort_results(false)
                    .frame_interval(Duration::from_secs(5))
                    .reversed(reversed)
                    .preview_split(split)
            };
            let events = || {
                frame_events([
                    Ok(Event::MatchList(if reversed {
                        MatchListEvent::Down(1)
                    } else {
                        MatchListEvent::Up(1)
                    })),
                    Ok(Event::Select),
                ])
            };
            let mut fresh = options()
                .picker(StrRenderer)
                .with_interface(UncachedPreview::new_single(preview));
            fresh.push_batch(["alpha", "beta"]);
            fresh.filter().unwrap();
            let mut expected = TestTerminal::default();
            assert_eq!(
                fresh
                    .pick_with_terminal_io(events(), &mut expected)
                    .unwrap(),
                Some(&"beta"),
            );
            for borrowed in [false, true] {
                let picker = options()
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
                let mut terminal = TestTerminal::default();
                let selected = if borrowed {
                    picker
                        .as_interface(Single)
                        .with_interface(UncachedPreview::new_single(preview))
                        .pick_with_terminal_io(events(), &mut terminal)
                } else {
                    picker.pick_with_terminal_io(events(), &mut terminal)
                }
                .unwrap();
                assert_eq!(selected, Some(&"beta"));
                assert_eq!(terminal.output, expected.output, "borrowed={borrowed}");
            }
        }
    }
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
            .frame_interval(Duration::from_millis(10))
            .background_frame_interval(Duration::from_millis(25))
            .picker::<&str, _>(StrRenderer);
        let _injector = injecting.then(|| picker.injector());
        picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        assert_eq!(terminal.size_reads, calls);
    }
}

#[test]
fn terminal_size_is_queried_after_preview_generation() {
    struct ResizingTerminal<'a>(&'a Cell<Dimensions>);

    impl io::Write for ResizingTerminal<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Terminal for ResizingTerminal<'_> {
        fn init(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn cleanup(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn dimensions(&mut self) -> io::Result<Dimensions> {
            Ok(self.0.get())
        }
    }

    let size = Cell::new(Dimensions {
        width: 20,
        height: 10,
    });
    let resized = Dimensions {
        width: 30,
        height: 12,
    };
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer)
        .with_interface(UncachedPreview::new_single(
            |item: &&str, pane: &mut Pane| {
                if *item == "beta" {
                    size.set(resized);
                }
                pane.push_text(item);
            },
        ));
    picker.push_batch(["alpha", "beta"]);
    picker.filter().unwrap();
    let statuses = picker.status_observer();
    picker
        .pick_with_terminal_io(
            frame_events([
                Ok(Event::MatchList(MatchListEvent::Up(1))),
                Ok(Event::Status { id: 0 }),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]),
            &mut ResizingTerminal(&size),
        )
        .unwrap();
    assert_eq!(statuses.try_recv().unwrap().dimensions, resized);
}
