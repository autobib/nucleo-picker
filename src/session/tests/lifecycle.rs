use std::{
    cell::RefCell,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use super::{FrameEvents, frame_events};
use crate::{
    Picker, PickerOptions, Terminal,
    component::PreviewComponent,
    event::{Event, RecvError},
    match_list::Queued,
    render::StrRenderer,
    session::{Context, Dimensions, Exit},
};

type Log = RefCell<Vec<&'static str>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanicAt {
    Never,
    Init,
    Update,
    Events,
    End,
    Finish,
}

struct Probe<'a> {
    log: &'a Log,
    panic_at: PanicAt,
    sessions: usize,
}

struct State<'a> {
    log: &'a Log,
    session: usize,
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        self.log.borrow_mut().push("drop state");
    }
}

impl<'a, T> PreviewComponent<T> for Probe<'a> {
    type State = State<'a>;

    fn init(&mut self) -> Self::State {
        self.log.borrow_mut().push("init");
        assert_ne!(self.panic_at, PanicAt::Init);
        self.sessions += 1;
        State {
            log: self.log,
            session: self.sessions,
        }
    }

    fn update(&mut self, state: &mut Self::State, _: &Context<'_, T>, _: bool, _: Instant) -> bool {
        assert_eq!(state.session, self.sessions);
        assert_ne!(self.panic_at, PanicAt::Update);
        false
    }

    fn restart(&mut self, state: &mut Self::State) {
        assert_eq!(state.session, self.sessions);
        self.log.borrow_mut().push("restart");
    }

    fn end(&mut self, state: &mut Self::State) {
        assert_eq!(state.session, self.sessions);
        self.log.borrow_mut().push("end");
        assert_ne!(self.panic_at, PanicAt::End);
    }
}

struct OutputProbe<'a> {
    log: &'a Log,
    panic_at: PanicAt,
}

impl Queued for OutputProbe<'_> {
    type Output<'items, T: Send + Sync + 'static> = Option<&'items T>;

    fn on_select<'items, T: Send + Sync + 'static>(
        self,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items, T> {
        assert_eq!(self.log.borrow().last(), Some(&"drop state"));
        self.log.borrow_mut().push("finish");
        assert_ne!(self.panic_at, PanicAt::Finish);
        ().on_select(exit, cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    None,
    Init,
    Dimensions,
    Write,
    EndFrame,
    Cleanup,
}

struct TestTerminal<'a> {
    log: &'a Log,
    failure: Failure,
}

impl TestTerminal<'_> {
    fn check(&self, phase: Failure) -> io::Result<()> {
        if self.failure == phase {
            Err(io::Error::other("terminal failure"))
        } else {
            Ok(())
        }
    }
}

impl io::Write for TestTerminal<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check(Failure::Write)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for TestTerminal<'_> {
    fn init(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("terminal init");
        self.check(Failure::Init)
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        self.check(Failure::Dimensions)?;
        Ok(Dimensions {
            width: 20,
            height: 8,
        })
    }

    fn end_frame(&mut self, _: bool) -> io::Result<()> {
        self.check(Failure::EndFrame)
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("terminal cleanup");
        self.check(Failure::Cleanup)
    }
}

fn picker() -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    picker
}

#[test]
fn cleanup_runs_once_before_output_and_on_terminal_errors() {
    let log = Log::default();
    let mut probe = Probe {
        log: &log,
        panic_at: PanicAt::Never,
        sessions: 0,
    };
    let mut picker = picker();
    for (session, failure) in [
        Failure::None,
        Failure::Init,
        Failure::Dimensions,
        Failure::Write,
        Failure::EndFrame,
        Failure::Cleanup,
    ]
    .into_iter()
    .enumerate()
    {
        for (select, exit) in [(true, Event::Select), (false, Event::Quit)] {
            log.borrow_mut().clear();
            let result = picker.core.pick_impl(
                frame_events([Err(RecvError::Timeout), Ok(exit)]),
                &mut TestTerminal { log: &log, failure },
                OutputProbe {
                    log: &log,
                    panic_at: PanicAt::Never,
                },
                &mut probe,
            );
            let mut expected = vec![
                "init",
                "terminal init",
                "terminal cleanup",
                "end",
                "drop state",
            ];
            if failure == Failure::None {
                assert_eq!(result.unwrap().copied(), select.then_some("alpha"));
                expected.push("finish");
            } else {
                assert!(matches!(result, Err(crate::error::PickError::IO(_))));
            }
            assert_eq!(*log.borrow(), expected, "{failure:?}, select={select}");
        }
        assert_eq!(probe.sessions, 2 * (session + 1));
    }
}

#[test]
fn event_errors_end_the_session_without_output_conversion() {
    for exit in [
        Ok(Event::UserInterrupt),
        Ok(Event::Abort("abort")),
        Err(RecvError::Disconnected),
        Err(RecvError::IO(io::Error::other("read failed"))),
    ] {
        let log = Log::default();
        let mut picker = picker();
        let result = picker.core.pick_impl(
            frame_events([exit]),
            &mut TestTerminal {
                log: &log,
                failure: Failure::None,
            },
            OutputProbe {
                log: &log,
                panic_at: PanicAt::Never,
            },
            &mut Probe {
                log: &log,
                panic_at: PanicAt::Never,
                sessions: 0,
            },
        );
        assert!(result.is_err());
        assert_eq!(
            *log.borrow(),
            [
                "init",
                "terminal init",
                "terminal cleanup",
                "end",
                "drop state"
            ]
        );
    }
}

#[test]
fn restart_preserves_state_until_session_cleanup() {
    let log = Log::default();
    let mut picker = picker();
    let _injectors = picker.injector_observer(false);
    picker
        .core
        .pick_impl(
            frame_events([Ok(Event::Restart), Ok(Event::Restart), Ok(Event::Quit)]),
            &mut TestTerminal {
                log: &log,
                failure: Failure::None,
            },
            OutputProbe {
                log: &log,
                panic_at: PanicAt::Never,
            },
            &mut Probe {
                log: &log,
                panic_at: PanicAt::Never,
                sessions: 0,
            },
        )
        .unwrap();
    assert_eq!(
        *log.borrow(),
        [
            "init",
            "terminal init",
            "restart",
            "restart",
            "terminal cleanup",
            "end",
            "drop state",
            "finish",
        ]
    );
}

#[test]
fn unwinding_drops_state_without_reentering_lifecycle_hooks() {
    for panic_at in [
        PanicAt::Init,
        PanicAt::Update,
        PanicAt::Events,
        PanicAt::End,
        PanicAt::Finish,
    ] {
        let log = Log::default();
        let mut picker = picker();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = picker.core.pick_impl(
                FrameEvents(|| {
                    assert_ne!(panic_at, PanicAt::Events);
                    Ok(Event::Select)
                }),
                &mut TestTerminal {
                    log: &log,
                    failure: Failure::None,
                },
                OutputProbe {
                    log: &log,
                    panic_at,
                },
                &mut Probe {
                    log: &log,
                    panic_at,
                    sessions: 0,
                },
            );
        }));
        assert!(result.is_err(), "{panic_at:?}");
        let mut expected = vec!["init"];
        if panic_at != PanicAt::Init {
            expected.extend(["terminal init", "terminal cleanup"]);
            if matches!(panic_at, PanicAt::End | PanicAt::Finish) {
                expected.push("end");
            }
            expected.push("drop state");
            if panic_at == PanicAt::Finish {
                expected.push("finish");
            }
        }
        assert_eq!(*log.borrow(), expected, "{panic_at:?}");
    }
}
