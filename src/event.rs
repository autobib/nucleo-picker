//! # Event handling and keybinding
//!
//! This module defines the core events handled by the TUI, as well as the built-in handling of
//! keybindings.

mod bind;

use crossterm::event::Event as CrosstermEvent;

pub use crate::match_list::MatchListEvent;
pub use crate::prompt::PromptEvent;
pub use self::bind::keybind_default;

/// An event which controls the picker behaviour.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// Modify the prompt.
    Prompt(PromptEvent),
    /// Modify the list of matches.
    MatchList(MatchListEvent),
    /// Quit the picker (no selection).
    Quit,
    /// Quit the picker (no selection) if the prompt is empty.
    QuitPromptEmpty,
    /// Abort the picker (error).
    Abort,
    /// Resize the screen.
    Redraw,
    /// Quit the picker and select the given item.
    Select,
}

pub(crate) fn convert(event: CrosstermEvent) -> Option<Event> {
    match event {
        CrosstermEvent::Key(key_event) => keybind_default(key_event),
        CrosstermEvent::Resize(_, _) => Some(Event::Redraw),
        CrosstermEvent::Paste(contents) => Some(Event::Prompt(PromptEvent::Paste(contents))),
        _ => None,
    }
}
