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
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        }) => match code {
            KeyCode::Char('c') | KeyCode::Char('g') | KeyCode::Char('q') => Some(Event::Abort),
            KeyCode::Char('b') => Some(Event::MoveLeft),
            KeyCode::Char('f') => Some(Event::MoveRight),
            KeyCode::Char('h') => Some(Event::Delete),
            KeyCode::Char('k') | KeyCode::Char('p') => Some(Event::MoveUp),
            KeyCode::Char('j') | KeyCode::Char('n') => Some(Event::MoveDown),
            KeyCode::Char('a') => Some(Event::MoveToStart),
            KeyCode::Char('e') => Some(Event::MoveToEnd),
            _ => None,
        },
        CrosstermEvent::Key(KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
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
            KeyCode::End => Some(Event::MoveToEnd),
            KeyCode::Home => Some(Event::MoveToStart),
            KeyCode::Esc => Some(Event::Quit),
            _ => None,
        },
        CrosstermEvent::Resize(width, height) => Some(Event::Resize(width, height)),
        CrosstermEvent::Paste(contents) => Some(Event::Paste(contents)),
        _ => None,
    }
}
