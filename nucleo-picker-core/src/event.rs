//! # Component-specific picker events.
//!
//! This module contains events which modify the picker components.

pub use crate::{match_list::MatchListEvent, prompt::PromptEvent};

/// Events which target the preview pane.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewEvent {
    /// Refresh the preview for the hovered item.
    Refresh,
    /// Scroll to the left by `n` columns.
    Left(usize),
    /// Scroll to the right by `n` columns.
    Right(usize),
    /// Align the preview window with the first column.
    AlignLeft,
    /// Align the right edge of the preview window with the longest currently visible row, or to
    /// the first column if all of the rows fit.
    ///
    /// Note that this may result in a left movement if the preview pane is currently scrolled past
    /// the final visible row. If you want to align to the right *without any possible left movement*,
    /// use `PreviewEvent::Right(usize::MAX)`.
    AlignRight,
    /// Scroll towards the beginning of the buffer by `n` lines.
    Up(usize),
    /// Scroll towards the end of the buffer by `n` lines.
    Down(usize),
    /// Scroll towards the beginning of the buffer by `n` pages.
    PageUp(usize),
    /// Scroll towards the end of the buffer by `n` pages.
    PageDown(usize),
    /// Align the preview window to the first row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignTop,
    /// Align the preview window to the final row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignBottom,
    /// Toggle the presence of line numbers in an active preview pane.
    ToggleLineNumbers,
    /// Set visibility of line numbers in the active preview pane.
    ///
    /// This event has no default keybinding.
    SetLineNumbers(bool),
}
