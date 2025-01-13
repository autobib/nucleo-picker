//! # Picker with interactive restarts
//!
//! Generates a list of 100 random `u32`s to be selected. The user can either select
//! one of the options, or press `ctrl + r` to receive a new list of random integers.

use std::{
    convert::Infallible,
    io::{self, IsTerminal},
    process::exit,
    thread::spawn,
};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nucleo_picker::{
    event::{keybind_default, Event, StdinReader},
    render::DisplayRenderer,
    Picker,
};
use rand::{distributions::Standard, thread_rng, Rng};

fn main() -> io::Result<()> {
    let mut picker: Picker<u32, _> = Picker::new(DisplayRenderer);

    // initialize stderr and check that we can actually write to the screen
    let mut stderr = io::stderr().lock();
    if !stderr.is_terminal() {
        eprintln!("Failed to start picker: STDERR not interactive!");
        exit(1);
    }

    // get a restart observer, which will be sent the new injectors when the picker processes a
    // `Restart` event; we initialize with an injector so that items are sent immediately
    let observer = picker.injector_observer(true);

    // Create a thread to regenerate the number list in response to 'restart' events.
    //
    // Generating 100 random `u32`s is extremely fast so we do not need to worry about keeping
    // up with restart events. For slower and more resource-intensive item generation, in
    // practice one would regularly check the channel using `receiver.try_recv` for new
    // restart events to check if the current computation should be halted and restarted on a
    // new injector. See the `restart_ext` example for this implementation.
    spawn(move || {
        let mut rng = thread_rng();

        // block when waiting for new events, since we have nothing else to do. If the match does
        // not succeed, it means the channel dropped so we can shut down this thread.
        while let Ok(mut injector) = observer.recv() {
            // the restart event here is an injector for the picker; send the new items to the
            // injector every time we witness the event
            injector.extend((&mut rng).sample_iter(Standard).take(100));
        }
    });

    // Initialize an event source to watch for keyboard events.
    //
    // It is also possible to process restart events in the same thread used to process keyboard
    // events. However, if generating new items were to take a long time, we do not want to lag
    // user input and block watching for new keyboard events. In this specific example, it would
    // be fine.
    let event_source = StdinReader::new(move |key_event| match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('r'),
            ..
        } => {
            // we create the restart event on `ctrl + n`. since this invalidates existing
            // injectors, a new injector is immediately sent to the observer which we are watching
            // in the other thread
            Some(Event::<Infallible>::Restart)
        }
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
