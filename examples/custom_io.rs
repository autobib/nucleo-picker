use std::io;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nucleo_picker::{
    event::{keybind_default, Event, StdinReader},
    render::StrRenderer,
    Picker,
};

/// Keybindings which use the default keybindings, but instead of aborting on `ctrl + c`,
/// simply perform a normal quit action.
fn keybind_no_interrupt<T, R>(key_event: KeyEvent) -> Option<Event<T, R>> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('c'),
            ..
        } => Some(Event::Quit),
        e => keybind_default(e),
    }
}

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    let choices = vec![
        "Alvar Aalto",
        "Frank Lloyd Wright",
        "Zaha Hadid",
        "Le Corbusier",
    ];

    // populate the matcher using the convenience 'extend' implementation
    picker.extend(choices);

    // launch the interactive picker with the customized keybindings, and draw the picker on
    // standard output
    match picker.pick_with_io(
        StdinReader::new(keybind_no_interrupt),
        &mut std::io::stdout(),
    )? {
        Some(opt) => println!("Your preferred architect is: '{opt}'"),
        None => println!("No architect selected!"),
    }

    Ok(())
}
