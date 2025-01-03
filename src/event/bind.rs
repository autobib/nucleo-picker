use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Event, PromptEvent, SelectionEvent};

pub fn bind_default(key_event: KeyEvent) -> Option<Event> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Esc => Some(Event::Quit),
            KeyCode::Up => Some(Event::Selection(SelectionEvent::Up(1))),
            KeyCode::Down => Some(Event::Selection(SelectionEvent::Down(1))),
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
            KeyCode::Char('c') => Some(Event::Abort),
            KeyCode::Char('d') => Some(Event::QuitPromptEmpty),
            KeyCode::Char('r') => Some(Event::Selection(SelectionEvent::Reset)),
            KeyCode::Char('g' | 'q') => Some(Event::Quit),
            KeyCode::Char('k' | 'p') => Some(Event::Selection(SelectionEvent::Up(1))),
            KeyCode::Char('j' | 'n') => Some(Event::Selection(SelectionEvent::Down(1))),
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
