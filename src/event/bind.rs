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
    keybind_no_multi_passthrough(key_event)
        .or_else(keybind_multi_passthrough)
        .ok()
}

/// Keybindings which do not emit [multi-selection events](MatchListEvent#multi-selection-events).
///
/// You should only use these keybindings when you are not running in multi-selection mode. Note
/// that the default keybindings will still work even in non-multi-selection mode, since the
/// multi-selection events are simply ignored. The purpose of these keybindings is to slightly
/// reduce overhead by not even emitting the event at all.
///
/// In most cases, you should just use the [default keybindings](keybind_default) since the additional
/// overhead is quite minimal. Using these keybindings in multi-selection mode will mean that
/// multi-selection events are never emitted, which will prevent the user from making more than 1
/// selection regardless of support in the picker itself.
#[inline]
pub fn keybind_no_multi<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_no_multi_passthrough(key_event).ok()
}

/// A composable set of keybindings for multi-selection mode.
#[inline]
pub fn keybind_multi_passthrough<A>(key_event: KeyEvent) -> Result<Event<A>, KeyEvent> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Tab => Ok(Event::MatchList(MatchListEvent::ToggleDown(1))),
            KeyCode::BackTab => Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        } => match code {
            KeyCode::Char('-') => Ok(Event::MatchList(MatchListEvent::UnqueueAll)),
            KeyCode::Char('=') => Ok(Event::MatchList(MatchListEvent::QueueMatches)),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code: KeyCode::BackTab,
            ..
        } => Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        e => Err(e),
    }
}

/// A composable version of `keybind_no_multi`.
#[inline]
fn keybind_no_multi_passthrough<A>(key_event: KeyEvent) -> Result<Event<A>, KeyEvent> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Esc => Ok(Event::Quit),
            KeyCode::Up => Ok(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Down => Ok(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Left => Ok(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Right => Ok(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Home => Ok(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::End => Ok(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char(ch) => Ok(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::Backspace => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Ok(Event::Select),
            KeyCode::Delete => Ok(Event::Prompt(PromptEvent::Delete(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        } => match code {
            KeyCode::Char('c') => Ok(Event::UserInterrupt),
            KeyCode::Char('d') => Ok(Event::QuitPromptEmpty),
            KeyCode::Char('0') => Ok(Event::MatchList(MatchListEvent::Reset)),
            KeyCode::Char('g' | 'q') => Ok(Event::Quit),
            KeyCode::Char('k' | 'p') => Ok(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Char('j' | 'n') => Ok(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Char('b') => Ok(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Char('f') => Ok(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Char('a') => Ok(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::Char('e') => Ok(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char('h') => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Char('w') => Ok(Event::Prompt(PromptEvent::BackspaceWord(1))),
            KeyCode::Char('u') => Ok(Event::Prompt(PromptEvent::ClearBefore)),
            KeyCode::Char('o') => Ok(Event::Prompt(PromptEvent::ClearAfter)),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::ALT,
            code,
            ..
        } => match code {
            KeyCode::Char('f') => Ok(Event::Prompt(PromptEvent::WordLeft(1))),
            KeyCode::Char('b') => Ok(Event::Prompt(PromptEvent::WordRight(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code,
            ..
        } => match code {
            KeyCode::Char(ch) => Ok(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::BackTab => Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
            KeyCode::Backspace => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Ok(Event::MatchList(MatchListEvent::Down(1))),
            _ => Err(key_event),
        },
        _ => Err(key_event),
    }
}

/// Convert a crossterm event into an [`Event`], mapping key events with the giving key bindings.
pub fn convert_crossterm_event<A, F: FnMut(KeyEvent) -> Option<Event<A>>>(
    ct_event: CrosstermEvent,
    mut keybind: F,
) -> Option<Event<A>> {
    match ct_event {
        CrosstermEvent::Key(key_event) => keybind(key_event),
        CrosstermEvent::Resize(_, _) => Some(Event::Redraw),
        CrosstermEvent::Paste(contents) => Some(Event::Prompt(PromptEvent::Paste(contents))),
        _ => None,
    }
}
