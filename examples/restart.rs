//! # Picker with interactive restarts
//!
//! Generates a list of 100 random `u32`s to be selected. The user can either select
//! one of the options, or press `ctrl + n` to receive a new list of random integers.

use std::{
    io::{self, IsTerminal},
    process::exit,
    sync::mpsc::sync_channel,
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

    // Create a thread to regenerate the number list in response to 'restart' events.
    //
    // Generating 100 random `u32`s is extremely fast so we do not need to worry about keeping
    // up with restart events. For slower and more resource-intensive event generation, in
    // practice one would regularly check the channel using `receiver.try_recv` for new
    // restart events while feeding the picker with items to check if the current computation
    // should be halted and restarted on a new injector.
    let (sync_sender, receiver) = sync_channel(8);

    // immediately send an injector down the channel so that the screen renders with some choices;
    // this is infallible since we know that the receiver has not yet been dropped at this point
    let _ = sync_sender.send(picker.injector());

    spawn(move || {
        let mut rng = thread_rng();

        // block when waiting for new events, since we have nothing else to do. If the match does
        // not succeed, it means the channel dropped so we can shut down this thread.
        while let Ok(mut injector) = receiver.recv() {
            // the restart event here is an injector for the picker; send the new items to the
            // injector every time we witness the event
            injector.extend((&mut rng).sample_iter(Standard).take(100));
        }
    });

    // Initialize a source to watch for keyboard events.
    //
    // It would also be possible to generate the new events directly inside the closure,
    // instead of passing the events to a separate thread. However, if generating new
    // items were to take a long time, we do not want to lag user input and block watching
    // for new keyboard events. In this specific example, it would be fine.
    let event_source = StdinReader::new(move |key_event: KeyEvent| match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('n'),
            ..
        } => Some(Event::Restart::<_, _>(sync_sender.clone())),
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
