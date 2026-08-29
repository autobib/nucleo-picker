use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender},
    },
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use libghostty_vt::{
    Terminal as GhosttyTerminal, TerminalOptions,
    terminal::{Mode, SizeReportSize},
};
use nucleo_picker::{
    Picker, PickerOptions, Terminal as PickerTerminal,
    event::{Event, Observer, PickerStatus, PromptEvent},
    render::StrRenderer,
};
use oneshot::Receiver as OneshotReceiver;

use crate::{
    error::{Error, ErrorKind},
    snapshot::{PaneSnapshot, Snapshotter},
};

type PtyWrites = Rc<RefCell<Vec<u8>>>;

struct SnapshotRequest {
    result: oneshot::Sender<Result<PaneSnapshot, String>>,
}

#[derive(Clone)]
struct TerminalSize {
    packed: Arc<AtomicU32>,
}

impl TerminalSize {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            packed: Arc::new(AtomicU32::new(Self::pack(cols, rows))),
        }
    }

    fn get(&self) -> (u16, u16) {
        let packed = self.packed.load(Ordering::Relaxed);
        ((packed >> 16) as u16, packed as u16)
    }

    fn set(&self, cols: u16, rows: u16) {
        self.packed.store(Self::pack(cols, rows), Ordering::Relaxed);
    }

    fn pack(cols: u16, rows: u16) -> u32 {
        (u32::from(cols) << 16) | u32::from(rows)
    }
}

struct GhosttyBackend {
    terminal: GhosttyTerminal<'static, 'static>,
    snapshotter: Snapshotter<'static>,
    requested_size: TerminalSize,
    applied_size: (u16, u16),
    snapshot_requests: MpscReceiver<SnapshotRequest>,
}

impl GhosttyBackend {
    fn new(
        requested_size: TerminalSize,
        snapshot_requests: MpscReceiver<SnapshotRequest>,
    ) -> Result<Self, libghostty_vt::error::Error> {
        let (cols, rows) = requested_size.get();
        let mut terminal = GhosttyTerminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: 0,
        })?;
        terminal.set_mode(Mode::GRAPHEME_CLUSTER, true)?;
        terminal.on_size(|terminal| {
            Some(SizeReportSize {
                rows: terminal.rows().ok()?,
                columns: terminal.cols().ok()?,
                cell_width: 1,
                cell_height: 1,
            })
        })?;

        Ok(Self {
            terminal,
            snapshotter: Snapshotter::new()?,
            requested_size,
            applied_size: (cols, rows),
            snapshot_requests,
        })
    }

    pub fn register_pty(&mut self) -> Result<PtyWrites, libghostty_vt::error::Error> {
        let pty_writes = Rc::new(RefCell::new(Vec::new()));
        let callback_writes = Rc::clone(&pty_writes);
        self.terminal
            .on_pty_write(move |_, data| callback_writes.borrow_mut().extend_from_slice(data))?;
        Ok(pty_writes)
    }

    #[cfg_attr(not(test), expect(dead_code))]
    pub fn with_pty(
        requested_size: TerminalSize,
        snapshot_requests: MpscReceiver<SnapshotRequest>,
    ) -> Result<(Self, PtyWrites), libghostty_vt::error::Error> {
        let mut backend = Self::new(requested_size, snapshot_requests)?;
        let pty_writes = backend.register_pty()?;
        Ok((backend, pty_writes))
    }
}

impl Write for GhosttyBackend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.terminal.vt_write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl PickerTerminal for GhosttyBackend {
    fn init(&mut self) -> io::Result<()> {
        execute!(self, EnterAlternateScreen, EnableBracketedPaste)
    }

    fn cleanup(&mut self) -> io::Result<()> {
        execute!(self, DisableBracketedPaste, LeaveAlternateScreen)
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        let requested = self.requested_size.get();
        if requested != self.applied_size {
            self.terminal
                .resize(requested.0, requested.1, 1, 1)
                .map_err(io::Error::other)?;
            self.applied_size = requested;
        }
        Ok(requested)
    }

    fn end_frame(&mut self, _changed: bool) -> io::Result<()> {
        while let Ok(request) = self.snapshot_requests.try_recv() {
            let snapshot = self
                .snapshotter
                .capture(&self.terminal)
                .map_err(|error| error.to_string());
            let _ = request.result.send(snapshot);
        }
        Ok(())
    }
}

pub struct Driver {
    statuses: Observer<PickerStatus>,
    events: MpscSender<Event>,
    snapshot_requests: MpscSender<SnapshotRequest>,
    result: OneshotReceiver<Result<Vec<String>, ErrorKind>>,
    requested_size: TerminalSize,
    next_id: u64,
    last_status: Option<PickerStatus>,
}

impl Driver {
    pub fn start_with_options<T: Into<String>>(items: Vec<T>, options: PickerOptions) -> Self {
        Self::start_inner(items, options, false)
    }

    pub fn start_multi_with_options<T: Into<String>>(
        items: Vec<T>,
        options: PickerOptions,
    ) -> Self {
        Self::start_inner(items, options, true)
    }

    fn start_inner<T: Into<String>>(items: Vec<T>, options: PickerOptions, multi: bool) -> Self {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        let (events, event_source) = mpsc::channel();
        let (snapshot_request_tx, snapshot_request_rx) = mpsc::channel();
        let (result_tx, result) = oneshot::channel();
        let requested_size = TerminalSize::new(60, 16);
        let thread_size = requested_size.clone();

        let mut picker: Picker<String, _> = options
            .background_frame_interval(Duration::ZERO)
            .frame_interval(Duration::from_millis(5))
            .picker(StrRenderer);
        picker.push_batch(items);
        let statuses = picker.status_observer();

        thread::spawn(move || {
            let result = GhosttyBackend::new(thread_size, snapshot_request_rx)
                .map_err(ErrorKind::Terminal)
                .and_then(|mut terminal| {
                    if multi {
                        picker
                            .pick_multi_with_terminal_io(event_source, &mut terminal)
                            .map(|selection| selection.iter().cloned().collect())
                    } else {
                        picker
                            .pick_with_terminal_io(event_source, &mut terminal)
                            .map(|item| item.into_iter().cloned().collect())
                    }
                    .map_err(ErrorKind::Picker)
                });
            let _ = result_tx.send(result);
        });

        Self {
            events,
            statuses,
            snapshot_requests: snapshot_request_tx,
            result,
            requested_size,
            next_id: 0,
            last_status: None,
        }
    }

    pub fn send(&self, event: Event) -> Result<(), Error> {
        self.events.send(event).map_err(|_| {
            ErrorKind::Disconnected
                .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
        })
    }

    pub fn type_text(&self, text: &str) -> Result<(), Error> {
        for ch in text.chars() {
            self.send(Event::Prompt(PromptEvent::Insert(ch)))?;
        }
        Ok(())
    }

    fn next_status_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn wait_for(
        &mut self,
        timeout: Duration,
        mut predicate: impl FnMut(&PickerStatus) -> bool,
    ) -> Result<PickerStatus, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            let id = self.next_status_id();
            self.send(Event::Status { id })?;
            let status = loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let status = self.statuses.recv_timeout(remaining).map_err(|error| {
                    match error {
                        mpsc::RecvTimeoutError::Timeout => ErrorKind::Timeout,
                        mpsc::RecvTimeoutError::Disconnected => ErrorKind::Disconnected,
                    }
                    .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
                })?;
                self.last_status = Some(status.clone());
                if status.id >= id {
                    break status;
                }
            };
            if predicate(&status) {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(ErrorKind::Timeout
                    .with_driver_context(self.last_status.as_ref(), self.requested_size.get()));
            }
        }
    }

    pub fn wait_for_match_complete(
        &mut self,
        timeout: Duration,
        matched: u32,
        total: u32,
    ) -> Result<PickerStatus, Error> {
        self.wait_for(timeout, |status| {
            !status.changed
                && !status.matching
                && !status.injecting
                && status.matched_item_count == matched
                && status.item_count == total
        })
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        self.requested_size.set(cols, rows);
        self.send(Event::Redraw)
    }

    pub fn checkpoint(
        &mut self,
        timeout: Duration,
        name: impl Into<String>,
    ) -> Result<PaneSnapshot, Error> {
        let name = name.into();
        let result: Result<PaneSnapshot, Error> = (|| {
            self.wait_for(timeout, |status| {
                !status.changed && !status.matching && !status.injecting
            })?;
            let (result_tx, result_rx) = oneshot::channel();
            let request = SnapshotRequest { result: result_tx };
            self.snapshot_requests.send(request).map_err(|_| {
                ErrorKind::Disconnected
                    .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
            })?;
            let inspection = result_rx.recv_timeout(timeout).map_err(|error| {
                match error {
                    oneshot::RecvTimeoutError::Timeout => ErrorKind::Timeout,
                    oneshot::RecvTimeoutError::Disconnected => ErrorKind::Disconnected,
                }
                .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
            })?;
            inspection.map_err(|message| {
                ErrorKind::Inspection(message)
                    .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
            })
        })();
        result.map_err(|error| error.with_checkpoint(name))
    }

    pub fn set_dimensions(
        &mut self,
        timeout: Duration,
        width: u16,
        height: u16,
    ) -> Result<PickerStatus, Error> {
        self.resize(width, height)?;
        self.wait_for(timeout, |status| {
            (status.width, status.height) == (width, height)
        })
    }

    pub fn finish(self, timeout: Duration) -> Result<Vec<String>, Error> {
        let result = self.result.recv_timeout(timeout).map_err(|error| {
            match error {
                oneshot::RecvTimeoutError::Timeout => ErrorKind::Timeout,
                oneshot::RecvTimeoutError::Disconnected => ErrorKind::Disconnected,
            }
            .with_driver_context(self.last_status.as_ref(), self.requested_size.get())
        })?;
        result.map_err(|kind| {
            kind.with_driver_context(self.last_status.as_ref(), self.requested_size.get())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error as StdError,
        sync::mpsc::RecvTimeoutError,
        time::{Duration, Instant},
    };

    use libghostty_vt::screen::Screen;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn rendering_uses_the_final_terminal_column() -> Result<(), Box<dyn StdError>> {
        let mut driver = Driver::start_with_options(vec!["abcdefgh"], PickerOptions::new());
        driver.wait_for_match_complete(WAIT, 1, 1)?;
        driver.resize(7, 3)?;

        let snapshot = driver.checkpoint(WAIT, "final-column")?;
        assert!(!snapshot.text[0].ends_with(' '));
        assert!(snapshot.text[1].ends_with('─'));
        assert!(snapshot.row_flags.is_empty());

        driver.send(Event::Quit)?;
        assert!(driver.finish(WAIT)?.is_empty());
        Ok(())
    }

    #[test]
    fn status_synchronizes_rapid_events_and_overwrites() -> Result<(), Box<dyn StdError>> {
        let items = (0..24).map(|index| format!("item-{index:02}")).collect();
        let mut driver = Driver::start_with_options(items, PickerOptions::new());
        driver.wait_for_match_complete(WAIT, 24, 24)?;
        driver.send(Event::Prompt(PromptEvent::Insert('i')))?;
        driver.send(Event::Status { id: 100 })?;
        driver.send(Event::Prompt(PromptEvent::Insert('t')))?;
        driver.send(Event::Status { id: 101 })?;
        driver.send(Event::Prompt(PromptEvent::Insert('e')))?;
        driver.send(Event::Status { id: 102 })?;

        let deadline = Instant::now() + WAIT;
        let status = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let status = driver.statuses.recv_timeout(remaining)?;
            if status.id >= 102 {
                break status;
            }
        };
        assert_eq!(status.id, 102);
        assert_eq!(status.query, "ite");
        assert!(matches!(
            driver.statuses.recv_timeout(Duration::from_millis(1)),
            Err(RecvTimeoutError::Timeout)
        ));
        driver.send(Event::Quit)?;
        assert!(driver.finish(WAIT)?.is_empty());
        Ok(())
    }

    #[test]
    fn terminal_adapter_lifecycle_resize_write_and_end_frame() -> Result<(), Box<dyn StdError>> {
        let requested_size = TerminalSize::new(20, 8);
        let (snapshot_request_tx, snapshot_request_rx) = mpsc::channel();
        let (mut backend, pty_writes) =
            GhosttyBackend::with_pty(requested_size.clone(), snapshot_request_rx)?;

        backend.init()?;
        assert_eq!(backend.terminal.active_screen()?, Screen::Alternate);
        assert_eq!(backend.size()?, (20, 8));
        assert_eq!(
            (backend.terminal.cols()?, backend.terminal.rows()?),
            (20, 8)
        );

        backend.write_all(b"adapter-write")?;
        let (result_tx, result_rx) = oneshot::channel();
        snapshot_request_tx
            .send(SnapshotRequest { result: result_tx })
            .map_err(|_| "inspection channel disconnected")?;
        backend.end_frame(true)?;
        assert!(result_rx.recv_timeout(WAIT)??.cursor.position.unwrap().x > 0);

        requested_size.set(12, 4);
        assert_eq!(backend.size()?, (12, 4));
        assert_eq!(
            (backend.terminal.cols()?, backend.terminal.rows()?),
            (12, 4)
        );
        backend.terminal.vt_write(b"\x1b[18t");
        assert!(!pty_writes.borrow().is_empty());

        backend.cleanup()?;
        assert_eq!(backend.terminal.active_screen()?, Screen::Primary);
        Ok(())
    }
}
