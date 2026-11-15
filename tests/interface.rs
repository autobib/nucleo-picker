#![cfg(feature = "unstable-backend")]

use std::{
    cell::{Cell, RefCell},
    io,
    num::NonZero,
    time::{Duration, Instant},
};

use nucleo_picker::{
    DriverOptions, Interface, Multi, PickerOptions, Single, Terminal, UncachedPreview,
    event::{Event, EventSource, LayoutEvent, MatchListEvent, RecvError},
    preview::{Content, Pane, PreviewEvent},
    render::StrRenderer,
    session::{Context, Dimensions, Exit, ItemId, QueueStatus, Redraw},
};

#[derive(Default)]
struct TestTerminal {
    buffer: Vec<u8>,
    renders: Vec<String>,
    changed: Vec<bool>,
}

impl io::Write for TestTerminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
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
        Ok(())
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        Ok(Dimensions {
            width: 60,
            height: 8,
        })
    }

    fn end_render(&mut self) -> io::Result<()> {
        self.renders
            .push(String::from_utf8(std::mem::take(&mut self.buffer)).unwrap());
        Ok(())
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.changed.push(changed);
        Ok(())
    }
}

struct Script<I>(I);

impl<I: Iterator<Item = Result<Event<&'static str>, RecvError>>> EventSource for Script<I> {
    type AbortErr = &'static str;

    fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
        self.0.next().expect("event script exhausted")
    }
}

fn options() -> PickerOptions {
    PickerOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
        .frame_interval(Duration::from_secs(5))
}

#[test]
fn driver_output_outlives_a_temporary_interface() {
    struct Last<'a>(&'a Cell<bool>);

    impl Interface<String> for Last<'_> {
        type State = ();
        type Output<'a> = Option<&'a String>;

        fn init(&mut self, _: &Context<'_, String>) {}

        fn on_end(&mut self, _: &mut ()) {
            self.0.set(true);
        }

        fn finish<'a>(&mut self, (): (), exit: Exit, cx: Context<'a, String>) -> Self::Output<'a> {
            assert!(self.0.get());
            assert!(matches!(exit, Exit::Selected(Some(_))));
            cx.matches().iter().next_back().map(|item| item.value())
        }
    }

    let mut driver = DriverOptions::new().sort_results(false).driver(StrRenderer);
    driver.push_batch(["alpha", "beta"].map(str::to_owned));
    driver.filter().unwrap();
    let selected = {
        let ended = Cell::new(false);
        let mut interface = Last(&ended);
        let mut terminal = TestTerminal::default();
        driver
            .pick_with_terminal_io(
                &mut interface,
                Script([Ok(Event::Select)].into_iter()),
                &mut terminal,
            )
            .unwrap()
    };
    assert_eq!(selected.unwrap(), "beta");

    driver.update_query("^alpha$");
    driver.filter().unwrap();
    let selected = driver
        .pick_with_terminal_io(
            &mut Single,
            Script([Ok(Event::Select)].into_iter()),
            &mut TestTerminal::default(),
        )
        .unwrap();
    assert_eq!(selected.unwrap(), "alpha");
}

#[test]
fn selection_defaults_survive_picker_and_view_conversions() {
    const OPTIONS: PickerOptions = PickerOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
        .max_selection_count(NonZero::new(1));

    fn events() -> impl EventSource<AbortErr = &'static str> {
        Script(
            [
                Ok(Event::MatchList(MatchListEvent::QueueMatches)),
                Ok(Event::Select),
            ]
            .into_iter(),
        )
    }

    let mut picker = OPTIONS
        .picker(StrRenderer)
        .multi()
        .single()
        .with_interface(Single);
    picker.push_batch(["alpha", "beta"]);
    picker.filter().unwrap();
    let selected = picker
        .as_multi()
        .pick_with_terminal_io(events(), &mut TestTerminal::default())
        .unwrap();
    assert!(selected.into_iter().copied().eq(["alpha"]));

    let mut picker = picker.multi();
    let selected = picker
        .pick_with_terminal_io(events(), &mut TestTerminal::default())
        .unwrap();
    assert!(selected.into_iter().copied().eq(["alpha"]));

    let selected = picker
        .as_single()
        .with_interface(Multi::new())
        .pick_with_terminal_io(events(), &mut TestTerminal::default())
        .unwrap();
    assert!(selected.into_iter().copied().eq(["alpha", "beta"]));
}

#[cfg(feature = "preview-cache")]
#[test]
fn preview_defaults_survive_rewrapping_and_borrowed_conversions() {
    use nucleo_picker::preview::SyncPreviewer;

    for capacity in [NonZero::new(1), NonZero::new(2), None] {
        let generated = RefCell::new(Vec::new());
        let previewer = SyncPreviewer(|item: &&'static str, pane: &mut Pane| {
            generated.borrow_mut().push(*item);
            pane.push_text(item);
        });
        let options = DriverOptions::new()
            .threads(NonZero::new(1))
            .sort_results(false)
            .frame_interval(Duration::from_secs(5));
        let picker = PickerOptions::from(options)
            .max_selection_count(NonZero::new(1))
            .preview_cache_size(capacity)
            .picker(StrRenderer)
            .with_preview(previewer)
            .multi()
            .single();
        let (picker, previewer) = picker.into_parts();
        let mut picker = picker
            .with_preview(())
            .without_preview()
            .with_preview(previewer)
            .multi();
        picker.push_batch(["alpha", "beta"]);
        picker.filter().unwrap();

        let selected = picker
            .as_single()
            .multi()
            .pick_with_terminal_io(
                Script(
                    [
                        Ok(Event::MatchList(MatchListEvent::Up(1))),
                        Err(RecvError::Timeout),
                        Ok(Event::MatchList(MatchListEvent::Down(1))),
                        Err(RecvError::Timeout),
                        Ok(Event::MatchList(MatchListEvent::QueueMatches)),
                        Ok(Event::Select),
                    ]
                    .into_iter(),
                ),
                &mut TestTerminal::default(),
            )
            .unwrap();
        assert!(selected.into_iter().copied().eq(["alpha"]));
        let expected = if capacity == NonZero::new(1) {
            &["alpha", "beta", "alpha"][..]
        } else {
            &["alpha", "beta"][..]
        };
        assert_eq!(&*generated.borrow(), expected);
    }
}

struct Custom;

struct State {
    queued: ItemId,
    limit: NonZero<u32>,
    pane: Pane,
    frame: usize,
}

impl Interface<String> for Custom {
    type State = State;
    type Output<'items> = Option<&'items String>;

    const PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, String>) -> Self::State {
        State {
            queued: cx.matches().get(0).unwrap().id(),
            limit: NonZero::new(1).unwrap(),
            pane: Pane::new(),
            frame: 0,
        }
    }

    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        state.queued == id
    }

    fn queue_status(&self, state: &Self::State) -> Option<QueueStatus> {
        Some(QueueStatus {
            count: 1,
            limit: Some(state.limit),
        })
    }

    fn before_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, String>,
        _: bool,
        _: Instant,
    ) -> Redraw {
        match state.frame {
            0 => {
                state.pane.push_text("initial preview");
                Redraw::PREVIEW
            }
            1 => {
                state.queued = cx.matches().get(1).unwrap().id();
                state.limit = NonZero::new(2).unwrap();
                Redraw::QUEUE
            }
            _ => Redraw::NONE,
        }
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        _: &Context<'_, String>,
        _: Instant,
    ) -> Redraw {
        state.frame += 1;
        if state.frame == 2 {
            state.pane.clear();
            state.pane.push_text("after preview");
            Redraw::PREVIEW
        } else {
            Redraw::NONE
        }
    }

    fn preview_event(
        &mut self,
        state: &mut Self::State,
        _: PreviewEvent,
        cx: &Context<'_, String>,
    ) -> Redraw {
        state.queued = cx.matches().get(0).unwrap().id();
        state.limit = NonZero::new(1).unwrap();
        state.pane.clear();
        state.pane.push_text("event preview");
        Redraw::QUEUE | Redraw::PREVIEW
    }

    fn preview_content<'s>(&'s self, state: &'s Self::State) -> Content<'s> {
        Content::Ready(&state.pane)
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        matches!(exit, Exit::Selected(_)).then(|| cx.matches().item(state.queued).unwrap().value())
    }
}

#[test]
fn interface_hooks_redraw_only_the_components_they_change() {
    let mut picker = options().picker(StrRenderer);
    picker.push_batch(["alpha", "beta"].map(str::to_owned));
    picker.filter().unwrap();
    let mut terminal = TestTerminal::default();
    let output = picker
        .as_interface(Custom)
        .pick_with_terminal_io(
            Script(
                [
                    Err(RecvError::Timeout),
                    Err(RecvError::Timeout),
                    Ok(Event::Preview(PreviewEvent::Refresh)),
                    Err(RecvError::Timeout),
                    Err(RecvError::Timeout),
                    Ok(Event::Select),
                ]
                .into_iter(),
            ),
            &mut terminal,
        )
        .unwrap();
    assert_eq!(output.unwrap(), "alpha");
    assert_eq!(terminal.changed, [true, true, true, false]);
    let [initial, queue, preview, both] = terminal.renders.as_slice() else {
        panic!("expected initial, queue, preview, and combined renders");
    };
    assert!(initial.contains("initial preview"));
    assert!(queue.contains("(1/2)"));
    assert!(!queue.contains('╭'));
    assert!(preview.contains("after preview"));
    assert!(!preview.contains("alpha"));
    assert!(!preview.contains("2/2"));
    assert!(both.contains("(1/1)"));
    assert!(both.contains("event preview"));
    for rendered in [queue, both] {
        assert!(rendered.contains("alpha"));
        assert!(rendered.contains("beta"));
        assert_eq!(rendered.matches('┃').count(), 1);
    }
    for rendered in [queue, preview, both] {
        assert!(!rendered.contains('>'));
    }
}

#[test]
fn uncached_previews_follow_frames_and_revisits_instead_of_event_targets() {
    let generated = RefCell::new(Vec::new());
    let mut picker = options()
        .picker(StrRenderer)
        .with_interface(UncachedPreview::new(
            Single,
            |item: &&'static str, pane: &mut Pane| {
                generated.borrow_mut().push(*item);
                for _ in 0..10 {
                    pane.push_text(item);
                    pane.push_text("\n");
                }
            },
        ));
    picker.push_batch(["alpha", "beta"]);
    picker.filter().unwrap();
    let mut terminal = TestTerminal::default();
    let output = picker
        .pick_with_terminal_io(
            Script(
                [
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Up(1))),
                    Ok(Event::Preview(PreviewEvent::PageDown(1))),
                    Ok(Event::Preview(PreviewEvent::Refresh)),
                    Ok(Event::MatchList(MatchListEvent::Down(1))),
                    Err(RecvError::Timeout),
                    Ok(Event::Preview(PreviewEvent::Refresh)),
                    Ok(Event::Layout(LayoutEvent::TogglePreview)),
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Up(1))),
                    Ok(Event::Preview(PreviewEvent::Refresh)),
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Down(1))),
                    Ok(Event::Layout(LayoutEvent::TogglePreview)),
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Up(1))),
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Down(1))),
                    Err(RecvError::Timeout),
                    Ok(Event::Preview(PreviewEvent::Refresh)),
                    Ok(Event::Select),
                ]
                .into_iter(),
            ),
            &mut terminal,
        )
        .unwrap();
    assert_eq!(output, Some(&"alpha"));
    assert_eq!(&*generated.borrow(), &["alpha", "alpha", "beta", "alpha"]);
    assert_eq!(
        terminal.changed,
        [false, false, true, true, true, true, true]
    );
    assert_eq!(terminal.renders.len(), 6);
}

#[test]
fn explicit_uncached_interfaces_preserve_limits_and_borrowed_output() {
    let generated = RefCell::new(Vec::new());
    let mut picker = options().picker(StrRenderer);
    picker.push_batch(["alpha", "beta"].map(str::to_owned));
    picker.filter().unwrap();
    let mut terminal = TestTerminal::default();
    let output = {
        let mut generate = |item: &String, pane: &mut Pane| {
            generated.borrow_mut().push(item.clone());
            pane.push_text(item);
        };
        picker
            .as_interface(UncachedPreview::new(
                Multi::new().max_selection_count(NonZero::new(1)),
                &mut generate,
            ))
            .pick_with_terminal_io(
                Script(
                    [
                        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                        Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
                        Ok(Event::Select),
                    ]
                    .into_iter(),
                ),
                &mut terminal,
            )
            .unwrap()
    };
    assert!(output.iter().map(String::as_str).eq(["alpha"]));
    let selected = picker
        .as_interface(UncachedPreview::new(
            Single,
            |item: &String, pane: &mut Pane| {
                generated.borrow_mut().push(item.clone());
                pane.push_text(item);
            },
        ))
        .pick_with_terminal_io(Script([Ok(Event::Select)].into_iter()), &mut terminal)
        .unwrap()
        .unwrap();
    assert_eq!(selected, "beta");
    assert_eq!(&*generated.borrow(), &["alpha", "beta"]);

    assert_eq!(
        picker
            .pick_with_terminal_io(Script([Ok(Event::Select)].into_iter()), &mut terminal)
            .unwrap()
            .unwrap(),
        "beta",
    );
    assert_eq!(generated.borrow().len(), 2);
}
