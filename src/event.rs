use std::process::exit;

use std::io;

use crossbeam::channel::Receiver;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// The outcome after processing all of the events.
pub enum EventOutcome {
    Continue,
    UpdateQuery(bool),
    Select,
    Quit,
}

use crate::{MovementType, PickerState};

/// Process events from the event channel and return the outcome.
pub fn process_events(
    term: &mut PickerState,
    events: &Receiver<Event>,
) -> Result<EventOutcome, io::Error> {
    let mut update_query = false;
    let mut append = true;

    for event in events.try_iter() {
        match event {
            // Exit
            Event::Key(KeyEvent {
                modifiers: KeyModifiers::CONTROL,
                code: KeyCode::Char('c') | KeyCode::Char('C'),
                ..
            }) => {
                exit(1);
            }
            // start of line
            Event::Key(
                KeyEvent {
                    modifiers: KeyModifiers::CONTROL,
                    code: KeyCode::Char('e'),
                    ..
                }
                | KeyEvent {
                    modifiers: KeyModifiers::NONE,
                    code: KeyCode::Home,
                    ..
                },
            ) => term.shift(MovementType::Start),
            // end of line
            Event::Key(
                KeyEvent {
                    modifiers: KeyModifiers::CONTROL,
                    code: KeyCode::Char('a'),
                    ..
                }
                | KeyEvent {
                    modifiers: KeyModifiers::NONE,
                    code: KeyCode::End,
                    ..
                },
            ) => term.shift(MovementType::End),
            // another key press
            Event::Key(KeyEvent {
                kind: KeyEventKind::Press,
                code,
                ..
            }) => {
                match code {
                    KeyCode::Char(ch) => {
                        update_query = true;
                        // if the cursor is at the end, it means the character was appended
                        append &= term.query.cursor_at_end();
                        term.insert_char(ch);
                    }
                    KeyCode::Enter => return Ok(EventOutcome::Select),
                    KeyCode::Up => {
                        term.incr_selection();
                    }
                    KeyCode::Down => {
                        update_query = true;
                        term.decr_selection();
                    }
                    KeyCode::Left => {
                        term.shift(MovementType::Left);
                    }
                    KeyCode::Right => {
                        term.shift(MovementType::Right);
                    }
                    KeyCode::Backspace => {
                        update_query = true;
                        append = false;
                        term.delete_char();
                    }
                    KeyCode::Esc => return Ok(EventOutcome::Quit),
                    _ => {}
                }
            }
            Event::Resize(width, height) => {
                term.resize(width, height);
            }
            Event::Paste(contents) => {
                update_query = true;
                append &= term.query.cursor_at_end();
                term.paste(&contents);
            }
            _ => {}
        }
    }
    Ok(if update_query {
        EventOutcome::UpdateQuery(append)
    } else {
        EventOutcome::Continue
    })
}
