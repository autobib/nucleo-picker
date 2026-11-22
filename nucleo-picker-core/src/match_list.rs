//! # The list of match candidates
//!
//! ## Layout rules
//! ### Layout rules for vertical alignment
//!
//! The core layout rules (in decreasing order of priority) are as follows.
//!
//! 1. Respect the padding below and above, except when the rank is near 0.
//! 2. Render as much of the hovered item as possible.
//! 3. When the screen size increases, render new elements with lower index, and then elements
//!    with higher index.
//! 4. When the screen size decreases, delete whitespace, and then delete elements with higher
//!    index, and then elements with lower index.
//! 5. Change the location of the hovered item on the screen as little as possible.
//!
//! ### Layout rules for horizontal alignment of items
//!
//! 1. Multiline items must have the same amount of scroll for each line.
//! 2. Do not hide highlighted characters.
//! 3. Prefer to make the scroll as small as possible.
mod component;
mod draw;
mod item;
mod layout;
mod span;
mod state;
mod unicode;

#[cfg(test)]
mod tests;

pub(crate) use component::{MatchList, MatchListView};
pub(crate) use state::MatchListState;

/// An event that changes the hovered item and the selection queue.
///
/// # Multi-selection events
///
/// The following events are only handled by the built-in picker interfaces in [multiple selection mode](https://docs.rs/nucleo-picker/latest/nucleo_picker/interface/index.html#multiple-selections):
///
/// - [`ToggleUp`](MatchListEvent::ToggleUp)
/// - [`ToggleDown`](MatchListEvent::ToggleDown)
/// - [`QueueMatches`](MatchListEvent::QueueMatches)
/// - [`Unqueue`](MatchListEvent::Unqueue)
/// - [`UnqueueAll`](MatchListEvent::UnqueueAll)
///
/// In this case, the corresponding movements are conditional: they will only be performed if the
/// resulting (un)queue action is successful. This is relevant if the interface bounds selection count,
/// since additional items cannot be added to the selection queue if the bound is reached, in
/// which case the hovered item will not move.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchListEvent {
    /// Move the hovered item up `usize` items.
    Up(usize),
    /// Toggle the hovered item then move up `usize` items.
    ToggleUp(usize),
    /// Move the hovered item down `usize` items.
    Down(usize),
    /// Toggle the hovered item then move down `usize` items.
    ToggleDown(usize),
    /// Add the hovered item and `usize` items above to the item queue, moving the hover to the
    /// last queued item.
    ///
    /// This event has no default keybinding.
    ///
    /// This method is deprecated but still supported with the same behaviour in the interim.
    #[deprecated(since = "0.13.0", note = "Directional range queuing is deprecated")]
    QueueAbove(usize),
    /// Add the hovered item and `usize` items below to the item queue, moving the hover to the
    /// last queued item.
    ///
    /// This event has no default keybinding.
    ///
    /// This method is deprecated but still supported with the same behaviour in the interim.
    #[deprecated(since = "0.13.0", note = "Directional range queuing is deprecated")]
    QueueBelow(usize),
    /// Add all matching items to the item queue, preferring items with higher priority in the
    /// match-list.
    QueueMatches,
    /// Remove the hovered item from the item queue.
    ///
    /// This event has no default keybinding.
    Unqueue,
    /// Clear the item queue.
    UnqueueAll,
    /// Reset the hovered item to the start of the match list.
    Reset,
}

/// Internal layout and rendering configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MatchListConfig {
    /// Whether or not to do match highlighting.
    pub highlight: bool,
    /// Whether or not to print a bright background behind the entire line for the entire
    /// hovered item.
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
