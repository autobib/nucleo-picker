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
    session::{Context, Dimensions, Driver, DriverOptions, Exit, FrameOptions, Session},
};

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

fn driver(items: impl IntoIterator<Item = &'static str>) -> Driver<String, StrRenderer> {
    let mut driver = DriverOptions::new().sort_results(false).driver(StrRenderer);
    driver.push_batch(items.into_iter().map(str::to_owned));
    driver.filter().unwrap();
    driver
}

pub(crate) struct Select;

impl<T: Send + Sync + 'static> Interface<T> for Select {
    type State = ();
    type Output<'items> = Option<&'items T>;

    fn init(&mut self, _: &Context<'_, T>) {}

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Selected(Some(id)) => Some(cx.matches().item(id).unwrap().value()),
            Exit::Quit | Exit::Selected(None) => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct TestTerminal {
    pub(crate) output: Vec<u8>,
    size: Option<(u16, u16)>,
    size_reads: usize,
    initial_render_delay: Duration,
    initial_render_end: Option<Instant>,
    first_frame: Option<Instant>,
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
        self.initial_render_end.get_or_insert_with(Instant::now);
        Ok(())
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.first_frame.get_or_insert_with(Instant::now);
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
