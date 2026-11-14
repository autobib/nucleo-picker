use std::{
    io::{self, Write},
    sync::mpsc,
};

use crate::{Picker, error::PickError, event::Event, render::StrRenderer};

use super::Terminal;

enum Failure {
    Init,
    WritePanic,
    Cleanup,
    CleanupPanic,
}

#[derive(Default)]
struct TestTerminal {
    failure: Option<Failure>,
    cleanup_calls: usize,
}

impl Write for TestTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if matches!(self.failure, Some(Failure::WritePanic)) {
            panic!("terminal write panic");
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for TestTerminal {
    fn init(&mut self) -> io::Result<()> {
        if matches!(self.failure, Some(Failure::Init)) {
            Err(io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.cleanup_calls += 1;
        match self.failure {
            Some(Failure::Cleanup) => Err(io::ErrorKind::BrokenPipe.into()),
            Some(Failure::CleanupPanic) if self.cleanup_calls == 1 => {
                panic!("terminal cleanup panic");
            }
            _ => Ok(()),
        }
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        Ok((20, 8))
    }
}

#[test]
fn panic_cleans_up_the_supplied_terminal() {
    for (failure, cleanups) in [(Failure::WritePanic, 1), (Failure::CleanupPanic, 2)] {
        let mut terminal = TestTerminal {
            failure: Some(failure),
            ..Default::default()
        };
        let mut picker: Picker<String, _> = Picker::new(StrRenderer);
        let (sender, events) = mpsc::channel::<Event>();
        sender.send(Event::Quit).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = picker.pick_with_terminal_io(events, &mut terminal);
        }));
        assert!(panic.is_err());
        assert_eq!(terminal.cleanup_calls, cleanups);
    }
}

#[test]
fn cleanup_runs_once_and_propagates_errors() {
    for (failure, expected) in [
        (None, None),
        (Some(Failure::Init), Some(io::ErrorKind::PermissionDenied)),
        (Some(Failure::Cleanup), Some(io::ErrorKind::BrokenPipe)),
    ] {
        let mut terminal = TestTerminal {
            failure,
            ..Default::default()
        };
        let mut picker: Picker<String, _> = Picker::new(StrRenderer);
        let (sender, events) = mpsc::channel::<Event>();
        sender.send(Event::Quit).unwrap();

        let result = picker.pick_with_terminal_io(events, &mut terminal);
        match (result, expected) {
            (Ok(None), None) => {}
            (Err(PickError::IO(error)), Some(kind)) => assert_eq!(error.kind(), kind),
            (result, expected) => panic!("unexpected result {result:?}, expected {expected:?}"),
        }
        assert_eq!(terminal.cleanup_calls, 1);
    }
}
