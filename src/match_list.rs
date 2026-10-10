//! # The list of match candidates
//!
//! ## Layout rules
//! ### Layout rules for vertical alignment
//!
//! The core layout rules (in decreasing order of priority) are as follows.
//!
//! 1. Respect the padding below and above, except when the cursor is near 0.
//! 2. Render as much of the selection as possible.
//! 3. When the screen size increases, render new elements with lower index, and then elemenets
//!    with higher index.
//! 4. When the screen size decreases, delete whitespace, and then delete elements with higher
//!    index, and then elements with lower index.
//! 5. Change the location of the cursor on the screen as little as possible.
//!
//! ### Layout rules for horizontal alignment of items
//!
//! 1. Multi-line items must have the same amount of scroll for each line.
//! 2. Do not hide highlighted characters.
//! 3. Prefer to make the scroll as small as possible.
mod component;
mod draw;
mod item;
mod layout;
mod selection;
mod span;
mod state;
mod unicode;

#[cfg(test)]
mod tests;

pub(crate) use component::MatchList;
pub use selection::Selection;
pub(crate) use selection::{Queued, SelectedIndices};
pub(crate) use state::MatchListState;

/// An event that modifies the selection in the match list.
///
/// # Multi-selection events
///
/// The following events are only handled by the picker in [multiple selection mode](crate::Picker#multiple-selections):
/// - [`ToggleUp`](MatchListEvent::ToggleUp)
/// - [`ToggleDown`](MatchListEvent::ToggleDown)
/// - [`QueueAbove`](MatchListEvent::QueueAbove)
/// - [`QueueBelow`](MatchListEvent::QueueBelow)
/// - [`QueueMatches`](MatchListEvent::QueueMatches)
/// - [`Unqueue`](MatchListEvent::Unqueue)
/// - [`UnqueueAll`](MatchListEvent::UnqueueAll)
///
/// In this case, the corresponding movements are conditional: they will only be performed if the
/// resulting (un)queue action is successful. This is relevant if the [selection count is
/// bounded](crate::PickerOptions::max_selection_count) is bounded, since additional items cannot
/// be added to the selection queue if the bound is reached, in which case the cursor will not
/// move.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchListEvent {
    /// Move the selection up `usize` items.
    Up(usize),
    /// Toggle the selection then move up `usize` items.
    ToggleUp(usize),
    /// Move the selection down `usize` items.
    Down(usize),
    /// Toggle the selection then move down `usize` items.
    ToggleDown(usize),
    /// Add the current item and `usize` items above to the item queue, moving the cursor to the
    /// last selected item.
    ///
    /// This event has no default keybind.
    QueueAbove(usize),
    /// Add the current item and `usize` items below to the item queue, moving the cursor to the
    /// last selected item.
    ///
    /// This event has no default keybind.
    QueueBelow(usize),
    /// Add all matching items to the item queue, preferring items with higher score.
    QueueMatches,
    /// Remove the current item from the item queue.
    ///
    /// This event has no default keybind.
    Unqueue,
    /// Clear the item queue.
    UnqueueAll,
    /// Reset the selection to the start of the match list.
    Reset,
}

/// Internal layout and rendering configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MatchListConfig {
    /// Whether or not to do match highlighting.
    pub highlight: bool,
    /// Whether or not to highlight the entire line.
    pub highlight_line: bool,
    /// Whether or not the screen is reversed.
    pub reversed: bool,
    /// The amount of padding around highlighted matches.
    pub highlight_padding: u16,
    /// The amount of padding when scrolling.
    pub scroll_padding: u16,
}

impl MatchListConfig {
    pub const fn new() -> Self {
        Self {
            highlight: true,
            highlight_line: false,
            reversed: false,
            highlight_padding: 3,
            scroll_padding: 3,
        }
    }
}

impl Default for MatchListConfig {
    fn default() -> Self {
        Self::new()
    }
}
