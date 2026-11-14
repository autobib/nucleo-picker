use std::{io, time::Duration};

use crate::{
    Picker,
    event::{Event, EventSource, RecvError},
    render::StrRenderer,
};

use super::Terminal;

struct ScriptedEvents<I>(I);

impl<I: Iterator<Item = Result<Event, RecvError>>> EventSource for ScriptedEvents<I> {
    type AbortErr = !;

    fn try_recv(&mut self) -> Result<Event, RecvError> {
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event, RecvError> {
        self.0.next().expect("event script exhausted")
    }
}

struct CountingTerminal {
    size: (u16, u16),
    size_calls: usize,
}

impl io::Write for CountingTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for CountingTerminal {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        self.size_calls += 1;
        Ok(self.size)
    }
}

#[test]
fn size_queries_follow_redraws_and_background_frequency() {
    for (name, redraw, frames, injecting, calls) in [
        ("idle", false, 1, false, 1),
        ("redraw", true, 1, false, 2),
        ("background", false, 7, true, 3),
    ] {
        let events = ScriptedEvents(
            redraw
                .then_some(Ok(Event::Redraw))
                .into_iter()
                .chain((0..frames).map(|_| Err(RecvError::Timeout)))
                .chain([Ok(Event::Quit)]),
        );
        let mut terminal = CountingTerminal {
            size: (20, 8),
            size_calls: 0,
        };
        let mut picker: Picker<String, _> = crate::PickerOptions::new()
            .frame_interval(Duration::from_millis(1))
            .background_frame_interval(Duration::from_millis(3))
            .picker(StrRenderer);
        let _injector = injecting.then(|| picker.injector());
        picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        assert_eq!(terminal.size_calls, calls, "{name}");
    }
}
