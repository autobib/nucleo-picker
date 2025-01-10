//! # A version of the `fzf` clone with better error handling
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
//!
//! Unlike the `fzf` example, this example forwards IO errors to the picker thread and tells it to
//! disconnect.
use std::{
    fmt,
    io::{self, IsTerminal},
    process::exit,
    sync::mpsc::channel,
    thread::spawn,
};

use nucleo_picker::{
    error::PickError,
    event::{Event, StdinEventSender},
    render::StrRenderer,
    Picker,
};

/// The custom error type for our application. We could just use an `io::Error` directly since it
/// already satisfies all of the trait bounds required by an `EventSource`, but for demonstration
/// purposes we also add more context to the IO error with a custom wrapper type.
#[derive(Debug)]
enum AppError {
    Key(io::Error),
    Stdin(io::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Key(io_err) => write!(f, "IO error during keyboard input: {io_err}"),
            AppError::Stdin(io_err) => {
                write!(f, "IO error when reading from standard input: {io_err}")
            }
        }
    }
}

// We must implement `std::error::Error` to satisfy the abort error type bounds
impl std::error::Error for AppError {}

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    // initialize stderr and check that we can actually write to the screen
    let mut stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        eprintln!("Failed to start picker: STDERR not interactive!");
        exit(1);
    }

    // create a new channel. The `sender` end is used to send `Event`s to the picker, and the
    // `receiver` end is passed directly to the picker so that it can receiv the corresponding
    // events
    let (sender, receiver) = channel();

    // spawn a stdin watcher to read keyboard events and send them to the channel
    let stdin_watcher = StdinEventSender::with_default_keybindings(sender.clone());
    spawn(move || match stdin_watcher.watch() {
        Ok(()) => {
            // this path occurs when the picker quits and the receiver is dropped so there
            // is no more work to be done
        }
        Err(io_err) => {
            // we received an IO error while trying to read keyboard events, so we recover the
            // inner channel and send an `Abort` event to tell the picker to quit immediately
            //
            // if we do not send the `Abort` event, or any other event which causes the picker to
            // quit (such as a `Quit` event), the picker will hang until the thread reading from
            // standard input completes, which could be a very long time
            let inner = stdin_watcher.into_sender();
            // if this fails, the picker already quit
            let _ = inner.send(Event::Abort(AppError::Key(io_err)));
            return;
        }
    });

    // spawn a reader to read lines from standard input
    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            for line in stdin.lines() {
                match line {
                    // add the line to the match list
                    Ok(s) => injector.push(s),
                    Err(io_err) => {
                        // if we encounter an IO error, we send the corresponding error
                        // to the picker so that it can abort and propogate the error
                        //
                        // here, it is also safe to simply ignore the IO error since the picker will
                        // remain interactive with the items it has already received.
                        let _ = sender.send(Event::Abort(AppError::Stdin(io_err)));
                        return;
                    }
                }
            }
        }
    });

    match picker.pick_with_io(receiver, &mut stderr) {
        Ok(Some(it)) => println!("{it}"),
        Ok(None) => exit(1),
        Err(e) => {
            match e {
                PickError::UserInterrupted => eprintln!("keyboard interrupt!"),
                PickError::Aborted(e) => {
                    // this is our custom error type, `AppError`, which we sent in one of the error
                    // paths in the other threads
                    eprintln!("{e}");
                }
                // the next three match arms could be omitted
                PickError::IO(io_err) => return Err(io_err),
                PickError::Disconnected => {
                    unreachable!("Abort always event sent before closing the channel")
                }
                PickError::NotInteractive => {
                    unreachable!("Interactive checks not performed with `pick_with_io`")
                }
                // we cannot match exhaustively; since the error implements `std::error::Error` we
                // can just print it.
                e => eprintln!("{e}"),
                // Another option is to instead propagate as an io error
                // e => return Err(e.into()),
            }
            exit(1);
        }
    }
    Ok(())
}
