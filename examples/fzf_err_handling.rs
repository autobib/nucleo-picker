//! # A version of the `fzf` clone with better error handling
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
    event::{Event, StdinEventSender},
    render::StrRenderer,
    Picker,
};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    // initialize stderr and check that we can actually write to the screen
    let mut stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        eprintln!("Failed to start picker: STDERR not interactive!");
        exit(1);
    }

    let (sender, receiver) = channel();

    // spawn a stdin watcher to read keyboard events and send them to the channel
    let stdin_watcher = StdinEventSender::with_default_keybindings(sender.clone());
    spawn(move || match stdin_watcher.watch() {
        Ok(()) => {}
        Err(_io_err) => {
            // destroy the sender and
            let inner = stdin_watcher.into_sender();
            let _ = inner.send(Event::Abort);
            return;
        }
    });

    // spawn a reader to read lines from Stdin
    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            for line in stdin.lines() {
                match line {
                    Ok(s) => injector.push(s),
                    Err(_err) => {
                        // tell the picker to abort if we encounter an IO error
                        let _ = sender.send(Event::Abort);
                        return;
                    }
                }
            }
        }
    });

    match picker.pick_with_io(receiver, &mut stderr)? {
        Some(it) => println!("{it}"),
        None => exit(1),
    }
    Ok(())
}
