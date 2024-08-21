//! # Event handling and keybind definitions
//! In this module, we define the key bindings used by the TUI and also handle other events.
//! Internally, we represent an event as an [`Event`]. To handle this, we convert from
//! [`crossterm::event::Event`] with the [`convert`] method.
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// A possible action that a component might handle.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveToStart,
    MoveToEnd,
    Delete,
    Quit,
    Abort,
    Resize(u16, u16),
    Insert(char),
    Select,
    Paste(String),
}

/// Convert any [`crossterm::event::Event`] that we handle.
pub fn convert(event: CrosstermEvent) -> Option<Event> {
    match event {
        CrosstermEvent::Key(KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('c') | KeyCode::Char('C'),
            ..
        }) => Some(Event::Abort),
        CrosstermEvent::Key(
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
        ) => Some(Event::MoveToStart),
        CrosstermEvent::Key(
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
        ) => Some(Event::MoveToEnd),
        CrosstermEvent::Key(KeyEvent {
            kind: KeyEventKind::Press,
            code,
            ..
        }) => match code {
            KeyCode::Char(ch) => Some(Event::Insert(ch)),
            KeyCode::Enter => Some(Event::Select),
            KeyCode::Up => Some(Event::MoveUp),
            KeyCode::Down => Some(Event::MoveDown),
            KeyCode::Left => Some(Event::MoveLeft),
            KeyCode::Right => Some(Event::MoveRight),
            KeyCode::Backspace => Some(Event::Delete),
            KeyCode::Esc => Some(Event::Quit),
            _ => None,
        },
        CrosstermEvent::Resize(width, height) => Some(Event::Resize(width, height)),
        CrosstermEvent::Paste(contents) => Some(Event::Paste(contents)),
        _ => None,
    }
}
