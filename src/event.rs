//! # Event handling and keybinding
//!
//! This module defines the core events handled by the TUI, as well as the built-in handling of
//! keybindings.

mod bind;
mod debounced;

use crossterm::event::Event as CrosstermEvent;

/// An event that modifies the prompt.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PromptEvent {
    /// Move the cursor `usize` graphemes to the left.
    Left(usize),
    /// Move the cursor `usize` Unicode words to the left.
    WordLeft(usize),
    /// Move the cursor `usize` graphemes to the right.
    Right(usize),
    /// Move the cursor `usize` Unicode words to the right.
    WordRight(usize),
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the start.
    ToEnd,
    /// Delete `usize` graphemes immediately preceding the cursor.
    Backspace(usize),
    /// Delete `usize` graphemes immediately following the cursor.
    Delete(usize),
    /// Delete `usize` Unicode words immediately preceding the cursor.
    BackspaceWord(usize),
    /// Clear everything before the cursor.
    ClearBefore,
    /// Clear everything after the cursor.
    ClearAfter,
    /// Insert a character at the cursor position.
    Insert(char),
    /// Paste a string at the cursor position.
    Paste(String),
}

/// An event that modifies the selection in the match list.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectionEvent {
    /// Move the selection up `usize` items.
    Up(usize),
    /// Move the selection down `usize` items.
    Down(usize),
    /// Reset the selection to the start of the match list.
    Reset,
}

/// An event which controls the picker behaviour.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// Modify the prompt.
    Prompt(PromptEvent),
    /// Modify the list of matches.
    Selection(SelectionEvent),
    /// Quit the picker (no selection).
    Quit,
    /// Quit the picker (no selection) if the prompt is empty.
    QuitPromptEmpty,
    /// Abort the picker (error).
    Abort,
    /// Resize the screen.
    Resize(u16, u16),
    /// Quit the picker and select the given item.
    Select,
}

pub(crate) fn convert(event: CrosstermEvent) -> Option<Event> {
    match event {
        CrosstermEvent::Key(key_event) => bind::bind_default(key_event),
        CrosstermEvent::Resize(width, height) => Some(Event::Resize(width, height)),
        CrosstermEvent::Paste(contents) => Some(Event::Prompt(PromptEvent::Paste(contents))),
        _ => None,
    }
}
