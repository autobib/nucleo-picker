mod component;
mod state;

#[cfg(test)]
mod tests;

pub(crate) use component::Prompt;
use state::PromptView;
pub(crate) use state::{PromptData, PromptState};

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
    /// Move the cursor to the end.
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
    /// Insert a character at the cursor.
    Insert(char),
    /// Paste a string at the cursor.
    Paste(String),
    /// Reset the prompt to a new string and move the cursor to the end.
    Reset(String),
}

impl PromptEvent {
    /// Whether or not the event is a cursor movement that does not edit the prompt string.
    #[must_use]
    pub fn is_cursor_movement(&self) -> bool {
        matches!(
            &self,
            Self::Left(_)
                | Self::WordLeft(_)
                | Self::Right(_)
                | Self::WordRight(_)
                | Self::ToStart
                | Self::ToEnd
        )
    }
}

#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub padding: u16,
}

impl PromptConfig {
    pub const fn new() -> Self {
        Self { padding: 2 }
    }
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self::new()
    }
}
