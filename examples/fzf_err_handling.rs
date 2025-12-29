//! # A simplified version of the `fzf` clone with better error handling
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
//!
//! Unlike the `fzf` example, this example forwards IO errors to the picker thread and tells it to
//! disconnect.

use std::{
    io::{self, IsTerminal},
    process::exit,
    sync::mpsc::channel,
    thread::spawn,
};

use nucleo_picker::{
    Picker,
    event::{Event, StdinEventSender},
    render::StrRenderer,
};

/// The custom error type for our application. We could just use an `io::Error` directly (and also
/// benefit from free error propogation), but for demonstration purposes we also add more context
/// to the IO error with a custom wrapper type.
enum AppError {
    Key(io::Error),
    Stdin(io::Error),
}

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    // initialize stderr and check that we can actually write to the screen
    let mut stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        eprintln!("Failed to start picker: STDERR not interactive!");
        exit(1);
    }

    // create a new channel. The `sender` end is used to send `Event`s to the picker, and the
    // `receiver` end is passed directly to the picker so that it can receive the corresponding
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
                        // it would also be fine to return but not send an abort event
                        // since the picker will remain interactive with the items it has
                        // already received.
                        let _ = sender.send(Event::Abort(AppError::Stdin(io_err)));
                        return;
                    }
                }
            }
        }
    });

    match picker.pick_with_io(receiver, &mut stderr) {
        Ok(Some(item)) => {
            println!("{item}");
            Ok(())
        }
        Ok(None) => exit(1),
        Err(e) => {
            // the 'factor' convenience method splits the error into a
            // `Result<A, PickError<Infallible>>`; so we just need to handle our application error.
            match e.factor()? {
                AppError::Key(io_err) => eprintln!("IO error during keyboard input: {io_err}"),
                AppError::Stdin(io_err) => {
                    eprintln!("IO error when reading from standard input: {io_err}")
                }
            }
            exit(1);
        }
    }
}
