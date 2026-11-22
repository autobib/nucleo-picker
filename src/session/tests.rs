use std::{
    io, thread,
    time::{Duration, Instant},
};

#[cfg(feature = "unstable-backend")]
use crate::event::{Event, EventSource, RecvError};
use crate::{
    Render, Terminal,
    interface::Interface,
    render::StrRenderer,
    session::{Dimensions, FrameOptions, Model, ModelOptions, Session},
};

pub(crate) use crate::interface::Single as Select;

#[cfg(feature = "preview-cache")]
mod cached;
mod context;
#[cfg(feature = "unstable-backend")]
mod lifecycle;
#[cfg(all(feature = "preview-cache", feature = "unstable-backend"))]
mod preview;
#[cfg(feature = "unstable-backend")]
mod run;
mod selection;
mod session;
mod updates;

pub(crate) fn frame_options() -> FrameOptions {
    FrameOptions::new(Instant::now())
}

pub(crate) fn update<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>>(
    session: &mut Session<'_, T, R, I>,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session
        .update_matches(Duration::from_millis(5))
        .is_complete()
    {
        assert!(Instant::now() < deadline, "matching did not complete");
        thread::yield_now();
    }
}

fn model(items: impl IntoIterator<Item = &'static str>) -> Model<String, StrRenderer> {
    let mut model = ModelOptions::new().sort_results(false).model(StrRenderer);
    model.push_batch(items.into_iter().map(str::to_owned));
    model.filter().unwrap();
    model
}

#[derive(Default)]
pub(crate) struct TestTerminal {
    pub(crate) output: Vec<u8>,
    size: Option<(u16, u16)>,
    size_reads: usize,
    initial_render_delay: Duration,
    frame_changes: Vec<bool>,
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
        Ok(())
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.frame_changes.push(changed);
        Ok(())
    }
}

#[cfg(feature = "unstable-backend")]
struct TestEvents<F>(F);

#[cfg(feature = "unstable-backend")]
impl<F: FnMut(Duration) -> Result<Event, RecvError>> EventSource for TestEvents<F> {
    type AbortErr = !;

    fn recv_timeout(&mut self, duration: Duration) -> Result<Event, RecvError> {
        (self.0)(duration)
    }
}

#[cfg(feature = "unstable-backend")]
struct FrameEvents<F>(F);

#[cfg(feature = "unstable-backend")]
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

#[cfg(feature = "unstable-backend")]
fn frame_events(
    events: impl IntoIterator<Item = Result<Event<&'static str>, RecvError>>,
) -> impl EventSource<AbortErr = &'static str> {
    let mut events = events.into_iter();
    FrameEvents(move || events.next().expect("event script exhausted"))
}
