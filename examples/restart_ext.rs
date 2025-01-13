//! # Picker with interactive restarts and slow event generation
//!
//! Generates a list of 1000 random `u32`s to be selected but with intentionally introduced delay
//! to imitate 'work' being done in the background.
//!
//! The user can either select one of the options, or press `ctrl + n` to receive a new list of
//! random integers. Try pressing `ctrl + n` very rapidly; new numbers will be generated even before
//! the previous list has finished rendering.
//!
//! This is a more complex version of the `restart` example; it is better to start there first. The
//! only documentation here is for the changes relative to the `restart` example.

use std::{
    convert::Infallible,
    io::{self, IsTerminal},
    process::exit,
    sync::mpsc::TryRecvError,
    thread::{sleep, spawn},
    time::Duration,
};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nucleo_picker::{
    event::{keybind_default, Event, StdinReader},
    render::DisplayRenderer,
    Picker,
};
use rand::random;

/// Generate a random `u32` but with a lot of extra delay to imitate an expensive computation.
fn slow_random() -> u32 {
    sleep(Duration::from_millis(5));
    random()
}

fn main() -> io::Result<()> {
    let mut picker: Picker<u32, _> = Picker::new(DisplayRenderer);

    let mut stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        eprintln!("Failed to start picker: STDERR not interactive!");
        exit(1);
    }

    // Do not initialize with an injector since we will send it ourself
    let observer = picker.injector_observer(false);

    let injector = picker.injector();
    spawn(move || {
        const NUM_ITEMS: usize = 1000;

        // because of the delay, generating 1000 `u32`s will take about 5 seconds, which is way
        // too long to be done in a single frame. Therefore after generating each random number
        // we check for a restart event before continuing.

        // In this example, coming up with the check frequency is quite easy since we know the
        // delay is 5ms, which is approximately the frame interval. In practice, with
        // computation-heavy item generation, tuning the check frequency to happen approximately
        // twice per frame can be very challenging. Note that the `receiver.try_recv` call is very
        // cheap so it is better to err towards overeager checks than infrequent checks.

        // the current active injector
        let mut current_injector = injector;
        let mut remaining_items = NUM_ITEMS;

        loop {
            match observer.try_recv() {
                Ok(new_injector) => {
                    // we received a new injector so we should immediately start sending `u32`s to
                    // it instead
                    current_injector = new_injector;
                    remaining_items = NUM_ITEMS;
                }
                Err(TryRecvError::Empty) => {
                    if remaining_items > 0 {
                        // we still have remaining data to be sent; continue to send it to the
                        // picker
                        remaining_items -= 1;
                        current_injector.push(slow_random());
                    } else if let Ok(new_injector) = observer.recv() {
                        // we have sent all of the necessary data; but we cannot simply skip this
                        // branch or we will spin-loop and consume unnecessary CPU cycles. Instead,
                        // we should block and wait for the next restart event since we have
                        // nothing else to do. Once we receive the new injector, reset the state
                        // and begin generating again.
                        current_injector = new_injector;
                        remaining_items = NUM_ITEMS;
                    } else {
                        // observer.recv() returned an error, means the channel disconnected so we
                        // can shut down this thread
                        return;
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    // the channel disconnected so we can shut down this thread
                    return;
                }
            }
        }
    });

    let event_source = StdinReader::new(move |key_event| match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('n'),
            ..
        } => Some(Event::<Infallible>::Restart),
        e => keybind_default(e),
    });

    match picker.pick_with_io(event_source, &mut stderr)? {
        Some(num) => {
            println!("Your favourite number is: {num}");
            Ok(())
        }
        None => {
            println!("You didn't like any of the numbers!");
            exit(1);
        }
    }
}
