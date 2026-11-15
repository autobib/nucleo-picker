use std::{
    io, thread,
    time::{Duration, Instant},
};

use crate::{
    Terminal,
    event::{Event, EventSource, RecvError},
    session::Dimensions,
};

mod context;
mod lifecycle;
#[cfg(all(feature = "preview", feature = "unstable-backend"))]
mod preview;
mod run;
mod selection;
mod updates;

#[derive(Default)]
struct TestTerminal {
    output: Vec<u8>,
    size: Option<(u16, u16)>,
    size_reads: usize,
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

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        self.size_reads += 1;
        let (width, height) = self.size.unwrap_or((20, 8));
        Ok(Dimensions { width, height })
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
    type AbortErr = !;

    fn recv_timeout(&mut self, duration: Duration) -> Result<Event, RecvError> {
        (self.0)(duration)
    }
}

struct FrameEvents<F>(F);

impl<F: FnMut() -> Result<Event<&'static str>, RecvError>> EventSource for FrameEvents<F> {
    type AbortErr = &'static str;

    fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
        // Advance the script only on timed receives to preserve frame boundaries on redraws.
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
        (self.0)()
    }
}

fn frame_events(
    events: impl IntoIterator<Item = Result<Event<&'static str>, RecvError>>,
) -> impl EventSource<AbortErr = &'static str> {
    let mut events = events.into_iter();
    FrameEvents(move || events.next().expect("event script exhausted"))
}
