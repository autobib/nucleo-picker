//! # Timeout example
//!
//! This is similar to the `find` example, except with a 5s timeout for inactivity, if no
//! selection is made and there is no entered text.

use std::{
    borrow::Cow,
    env::args,
    io::{self, BufWriter, IsTerminal},
    path::PathBuf,
    process::exit,
    sync::mpsc::channel,
    thread::spawn,
    time::Instant,
};

use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{
    PickerOptions, Render,
    error::PickError,
    event::{Event, StdinEventSender},
};

// the number of milliseconds before timeout
const TIMEOUT_MILLIS: u64 = 5_000;

// a custom application error
pub enum AppError {
    // an error occured while iterating over directories
    FindFailed(io::Error),
    // the selection timed out
    TimedOut,
}

impl From<io::Error> for AppError {
    fn from(value: io::Error) -> Self {
        Self::FindFailed(value)
    }
}

pub struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Str<'a> = Cow<'a, str>;

    fn render<'a>(&self, value: &'a DirEntry) -> Self::Str<'a> {
        value.path().to_string_lossy()
    }
}

fn main() -> io::Result<()> {
    let mut picker = PickerOptions::default()
        .match_paths()
        .picker(DirEntryRender);

    let root: PathBuf = match args().nth(1) {
        Some(path) => path.into(),
        None => ".".into(),
    };

    // populate the picker from a separate thread
    let injector = picker.injector();
    spawn(move || {
        WalkBuilder::new(root).build_parallel().run(|| {
            let injector = injector.clone();
            Box::new(move |walk_res| {
                if let Ok(dir) = walk_res {
                    injector.push(dir);
                }
                WalkState::Continue
            })
        });
    });

    // set up the event source: we mostly read from stdin into a channel, but we also
    // keep the handle to send `Event::Status` requests
    let (event_sender, event_receiver) = channel();
    let stdin_watcher = StdinEventSender::with_default_keybindings(event_sender.clone());
    spawn(move || {
        if let Err(io_err) = stdin_watcher.watch() {
            let _ = stdin_watcher
                .into_sender()
                .send(Event::Abort(AppError::FindFailed(io_err)));
        }
    });

    // set up the status handling
    let status_observer = picker.status_observer();
    spawn(move || {
        let start = Instant::now();
        let mut last_change = 0;

        loop {
            let id = start.elapsed().as_millis() as u64;
            if event_sender.send(Event::Status { id }).is_err() {
                // the picker shut down, so we can terminate this thread
                return;
            }

            // wait for the response. this is automatically debounced since the
            // picker only returns status responses once per frame
            let Ok(status) = status_observer.recv() else {
                return;
            };

            if status.changed {
                last_change = status.id;
            } else if status.selected_item_count == 0
                && status.query.is_empty()
                && status.id.saturating_sub(last_change) >= TIMEOUT_MILLIS
            {
                // no changes within timeout millis, plus no queued changes and no entered text: auto-quit
                let _ = event_sender.send(Event::Abort(AppError::TimedOut));
                return;
            }
        }
    });

    let stderr = io::stderr();
    if !stderr.is_terminal() {
        return Err(PickError::<io::Error>::NotInteractive.into());
    }
    let mut stderr = BufWriter::new(stderr.lock());

    match picker.pick_multi_with_io(event_receiver, &mut stderr) {
        Ok(selection) => {
            if selection.is_empty() {
                eprintln!("No path selected!");
                exit(1);
            }
            for entry in selection.iter() {
                println!("{}", entry.path().display());
            }
        }
        Err(err) => match err.factor()? {
            AppError::FindFailed(io_err) => return Err(io_err),
            AppError::TimedOut => {
                eprintln!("Selection timed out!");
            }
        },
    };

    Ok(())
}
