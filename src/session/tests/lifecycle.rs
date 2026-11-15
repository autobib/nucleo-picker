use std::{
    cell::{Cell, RefCell},
    io,
    num::NonZero,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use super::{FrameEvents, frame_events};
use crate::{
    Picker, PickerOptions, Terminal,
    error::PickError,
    event::{Event, RecvError},
    render::StrRenderer,
    session::{Context, Dimensions, Exit, Interface, Redraw, Single},
};

type Log = RefCell<Vec<&'static str>>;

struct Probe<'a> {
    log: &'a Log,
    panic_at: Option<&'static str>,
}

impl<'a> Probe<'a> {
    fn new(log: &'a Log) -> Self {
        Self {
            log,
            panic_at: None,
        }
    }

    fn call(&self, hook: &'static str) {
        self.log.borrow_mut().push(hook);
        if self.panic_at == Some(hook) {
            panic!("interface panic at {hook}");
        }
    }
}

struct State<'a>(&'a Log);

impl Drop for State<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().push("drop state");
    }
}

impl<'a, T: Send + Sync + 'static> Interface<T> for Probe<'a> {
    type State = State<'a>;
    type Output<'items> = Option<&'items T>;

    fn init(&mut self, _: &Context<'_, T>) -> Self::State {
        self.call("init");
        State(self.log)
    }

    fn on_restart(&mut self, _: &mut Self::State, _: &Context<'_, T>) {
        self.call("restart");
    }

    fn on_end(&mut self, _: &mut Self::State) {
        self.call("end");
    }

    fn finish<'items>(
        &mut self,
        _state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        self.call("finish");
        Single.finish((), exit, cx)
    }

    fn before_frame(
        &mut self,
        _: &mut Self::State,
        _: &Context<'_, T>,
        _: bool,
        _: Instant,
    ) -> Redraw {
        self.call("before");
        Redraw::NONE
    }

    fn after_frame(&mut self, _: &mut Self::State, _: &Context<'_, T>, _: Instant) -> Redraw {
        self.call("after");
        Redraw::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Error(&'static str, io::ErrorKind),
    Panic(&'static str),
}

struct TestTerminal<'a> {
    log: &'a Log,
    failure: &'a Cell<Option<Failure>>,
}

impl TestTerminal<'_> {
    fn check(&self, phase: &'static str) -> io::Result<()> {
        match self.failure.get() {
            Some(Failure::Error(at, kind)) if at == phase => Err(kind.into()),
            Some(Failure::Panic(at)) if at == phase => {
                self.failure.set(None);
                panic!("terminal panic at {phase}");
            }
            _ => Ok(()),
        }
    }
}

impl io::Write for TestTerminal<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check("write")?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for TestTerminal<'_> {
    fn init(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("terminal init");
        self.check("terminal init")
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        self.check("dimensions")?;
        Ok(Dimensions {
            width: 20,
            height: 8,
        })
    }

    fn end_frame(&mut self, _: bool) -> io::Result<()> {
        self.check("end frame")
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("terminal cleanup");
        self.check("terminal cleanup")
    }
}

fn picker() -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .threads(NonZero::new(1))
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    picker
}

#[test]
fn terminal_cleanup_precedes_interface_cleanup_and_output() {
    for failure in [
        None,
        Some(Failure::Error(
            "terminal init",
            io::ErrorKind::PermissionDenied,
        )),
        Some(Failure::Error("dimensions", io::ErrorKind::Other)),
        Some(Failure::Error("write", io::ErrorKind::WriteZero)),
        Some(Failure::Error(
            "terminal cleanup",
            io::ErrorKind::BrokenPipe,
        )),
    ] {
        let log = Log::default();
        let fault = Cell::new(failure);
        let mut picker = picker();
        let result = picker.as_interface(Probe::new(&log)).pick_with_terminal_io(
            frame_events([Ok(Event::Select)]),
            &mut TestTerminal {
                log: &log,
                failure: &fault,
            },
        );
        match (failure, result) {
            (None, Ok(selected)) => assert_eq!(selected, Some(&"alpha")),
            (Some(Failure::Error(_, kind)), Err(PickError::IO(error))) => {
                assert_eq!(error.kind(), kind);
            }
            (failure, result) => panic!("unexpected result {result:?} for {failure:?}"),
        }
        let mut expected = vec!["init", "terminal init"];
        if !matches!(failure, Some(Failure::Error("terminal init", _))) {
            expected.push("before");
        }
        if matches!(failure, None | Some(Failure::Error("terminal cleanup", _))) {
            expected.push("after");
        }
        expected.extend(["terminal cleanup", "end"]);
        if failure.is_none() {
            expected.push("finish");
        }
        expected.push("drop state");
        assert_eq!(*log.borrow(), expected, "{failure:?}");
    }
}

#[test]
fn event_errors_end_without_output_and_cleanup_errors_take_precedence() {
    for (exit, cleanup_fails) in [
        (Ok(Event::UserInterrupt), false),
        (Ok(Event::Abort("abort")), false),
        (Err(RecvError::Disconnected), false),
        (
            Err(RecvError::IO(io::ErrorKind::UnexpectedEof.into())),
            false,
        ),
        (
            Err(RecvError::IO(io::ErrorKind::UnexpectedEof.into())),
            true,
        ),
    ] {
        let log = Log::default();
        let fault = Cell::new(cleanup_fails.then_some(Failure::Error(
            "terminal cleanup",
            io::ErrorKind::BrokenPipe,
        )));
        let mut picker = picker();
        let error = picker
            .as_interface(Probe::new(&log))
            .pick_with_terminal_io(
                frame_events([exit]),
                &mut TestTerminal {
                    log: &log,
                    failure: &fault,
                },
            )
            .unwrap_err();
        if cleanup_fails {
            assert!(
                matches!(error, PickError::IO(error) if error.kind() == io::ErrorKind::BrokenPipe)
            );
        }
        assert_eq!(
            *log.borrow(),
            [
                "init",
                "terminal init",
                "before",
                "after",
                "terminal cleanup",
                "end",
                "drop state",
            ]
        );
    }
}

#[test]
fn failed_presentation_skips_status_and_after_frame() {
    for phase in ["write", "end frame"] {
        let log = Log::default();
        let fault = Cell::new(None);
        let mut picker = picker();
        let statuses = picker.status_observer();
        let mut events = [Event::Status { id: 7 }, Event::Redraw].into_iter();
        let result = picker.as_interface(Probe::new(&log)).pick_with_terminal_io(
            FrameEvents(|| {
                fault.set(Some(Failure::Error(phase, io::ErrorKind::BrokenPipe)));
                Ok(events.next().expect("expected presentation to fail"))
            }),
            &mut TestTerminal {
                log: &log,
                failure: &fault,
            },
        );
        assert!(
            matches!(result, Err(PickError::IO(error)) if error.kind() == io::ErrorKind::BrokenPipe)
        );
        assert!(matches!(statuses.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(
            *log.borrow(),
            [
                "init",
                "terminal init",
                "before",
                "after",
                "before",
                "terminal cleanup",
                "end",
                "drop state",
            ],
            "{phase}",
        );
    }
}

#[test]
fn restart_keeps_interface_state_until_session_cleanup() {
    let log = Log::default();
    let fault = Cell::new(None);
    let mut picker = picker();
    let _injectors = picker.injector_observer(false);
    let output = picker
        .as_interface(Probe::new(&log))
        .pick_with_terminal_io(
            frame_events([Ok(Event::Restart), Ok(Event::Restart), Ok(Event::Quit)]),
            &mut TestTerminal {
                log: &log,
                failure: &fault,
            },
        )
        .unwrap();
    assert_eq!(output, None);
    assert_eq!(
        *log.borrow(),
        [
            "init",
            "terminal init",
            "before",
            "after",
            "restart",
            "restart",
            "terminal cleanup",
            "end",
            "finish",
            "drop state",
        ]
    );
}

#[test]
fn panics_clean_up_the_terminal_without_reentering_interface_hooks() {
    for panic_at in [
        "init",
        "before",
        "after",
        "events",
        "end",
        "finish",
        "write",
        "terminal cleanup",
    ] {
        let log = Log::default();
        let fault = Cell::new(Some(Failure::Panic(panic_at)));
        let mut picker = picker();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = picker
                .as_interface(Probe {
                    log: &log,
                    panic_at: Some(panic_at),
                })
                .pick_with_terminal_io(
                    FrameEvents(|| {
                        assert_ne!(panic_at, "events");
                        Ok(Event::Select)
                    }),
                    &mut TestTerminal {
                        log: &log,
                        failure: &fault,
                    },
                );
        }));
        assert!(result.is_err(), "{panic_at}");
        let mut expected = vec!["init"];
        if panic_at != "init" {
            expected.extend(["terminal init", "before"]);
            if !matches!(panic_at, "before" | "write") {
                expected.push("after");
            }
            expected.push("terminal cleanup");
            if panic_at == "terminal cleanup" {
                expected.push("terminal cleanup");
            }
            if matches!(panic_at, "end" | "finish") {
                expected.push("end");
            }
            if panic_at == "finish" {
                expected.push("finish");
            }
            expected.push("drop state");
        }
        assert_eq!(*log.borrow(), expected, "{panic_at}");
    }
}
