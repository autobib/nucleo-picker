use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Event, MatchListEvent, PromptEvent};

/// The default keybindings.
///
/// These are the keybindings used in the [`Default`] implementation for
/// [`StdinReader`](super::StdinReader).
///
/// # Generic parameter
/// This function is generic over the type parameter `A`, which is the associated type
/// [`AbortErr`](super::EventSource::AbortErr) of an [`EventSource`](super::EventSource).
/// However, the type parameter does not appear anywhere in the function arguments since an
/// [`Event::Abort`] is never produced by the default keybindings. This type parameter is simply
/// here for flexibility to generate events of a particular type when used in situations where `A`
/// is not the default `!`.
#[inline]
pub fn keybind_default<A>(key_event: KeyEvent) -> Option<Event<A>> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Esc => Some(Event::Quit),
            KeyCode::Up => Some(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Down => Some(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Left => Some(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Right => Some(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Home => Some(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::End => Some(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char(ch) => Some(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::Backspace => Some(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Some(Event::Select),
            KeyCode::Delete => Some(Event::Prompt(PromptEvent::Delete(1))),
            _ => None,
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        } => match code {
            KeyCode::Char('c') => Some(Event::UserInterrupt),
            KeyCode::Char('d') => Some(Event::QuitPromptEmpty),
            KeyCode::Char('0') => Some(Event::MatchList(MatchListEvent::Reset)),
            KeyCode::Char('g' | 'q') => Some(Event::Quit),
            KeyCode::Char('k' | 'p') => Some(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Char('j' | 'n') => Some(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Char('b') => Some(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Char('f') => Some(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Char('a') => Some(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::Char('e') => Some(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char('h') => Some(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Char('w') => Some(Event::Prompt(PromptEvent::BackspaceWord(1))),
            KeyCode::Char('u') => Some(Event::Prompt(PromptEvent::ClearBefore)),
            KeyCode::Char('o') => Some(Event::Prompt(PromptEvent::ClearAfter)),
            _ => None,
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::ALT,
            code,
            ..
        } => match code {
            KeyCode::Char('f') => Some(Event::Prompt(PromptEvent::WordLeft(1))),
            KeyCode::Char('b') => Some(Event::Prompt(PromptEvent::WordRight(1))),
            _ => None,
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code,
            ..
        } => match code {
            KeyCode::Char(ch) => Some(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::Backspace => Some(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Some(Event::Select),
            _ => None,
        },
        _ => None,
    }
}

/// Convert a crossterm event into an [`Event`], mapping key events with the giving key bindings.
pub fn convert_crossterm_event<A, F: FnMut(KeyEvent) -> Option<Event<A>>>(
    ct_event: CrosstermEvent,
    mut keybind: F,
) -> Option<Event<A>> {
    match ct_event {
        CrosstermEvent::Key(key_event) => (keybind)(key_event),
        CrosstermEvent::Resize(_, _) => Some(Event::Redraw),
        CrosstermEvent::Paste(contents) => Some(Event::Prompt(PromptEvent::Paste(contents))),
        _ => None,
    }
}
