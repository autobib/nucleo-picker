use std::{collections::VecDeque, convert::Infallible, io, time::Duration};

use crate::{
    Picker,
    event::{Event, EventSource, RecvError},
    render::StrRenderer,
};

use super::Terminal;

enum Step {
    Event(Event),
    Timeout,
}

struct ScriptedEvents {
    steps: VecDeque<Step>,
}

impl ScriptedEvents {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }
}

impl EventSource for ScriptedEvents {
    type AbortErr = Infallible;

    fn recv_timeout(&mut self, _: Duration) -> Result<Event, RecvError> {
        match self.steps.pop_front().expect("event script exhausted") {
            Step::Event(event) => Ok(event),
            Step::Timeout => Err(RecvError::Timeout),
        }
    }
}

struct CountingTerminal {
    output: Vec<u8>,
    size: (u16, u16),
    size_calls: usize,
}

impl io::Write for CountingTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
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
fn idle_frame_does_not_query_terminal_size() {
    let events = ScriptedEvents::new([Step::Timeout, Step::Event(Event::Quit)]);
    let mut terminal = CountingTerminal {
        output: Vec::new(),
        size: (20, 8),
        size_calls: 0,
    };
    let mut picker: Picker<String, _> = Picker::new(StrRenderer);

    picker.pick_with_terminal_io(events, &mut terminal).unwrap();

    assert_eq!(terminal.size_calls, 1);
}

#[test]
fn redraw_queries_terminal_size() {
    let events = ScriptedEvents::new([
        Step::Event(Event::Redraw),
        Step::Timeout,
        Step::Event(Event::Quit),
    ]);
    let mut terminal = CountingTerminal {
        output: Vec::new(),
        size: (20, 8),
        size_calls: 0,
    };
    let mut picker: Picker<String, _> = Picker::new(StrRenderer);

    picker.pick_with_terminal_io(events, &mut terminal).unwrap();

    assert_eq!(terminal.size_calls, 2);
}
