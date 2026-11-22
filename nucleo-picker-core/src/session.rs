//! # Picker session
//!
//! A *session* is a single interactive call to the picker. The behaviour in a session is
//! determined by the [`Interface`](crate::interface::Interface) of the picker.
//!
//! The picker can be re-used for multiple sessions: the items and query are retained between
//! sessions (but can be cleared by restarting).
//!
//! Read more:
//!
//! - About interfaces in the [`interface`](crate::interface) module documentation.
//! - About the session API in the [`Session`] struct documentation.
//!
//! ## Session boundary
//!
//! In short, [`Session`] consists of the following components:
//!
//! - The [`Model`], which contains the item pool, query, and navigation state.
//! - The [`Interface`](crate::interface::Interface), representing external state which coordinates
//!   with the session.
//! - The [interface state](crate::interface::Interface::State), which persists only for a single
//!   session and includes the mutable state (such as an item queue for multiple selections).
//!
//! The caller drives a session to run a picker interface. Generally speaking, the following are the
//! responsibility of the runner:
//!
//! - Converting input into events.
//! - Advancing matching
//! - Preparing and drawing frames.
//! - Handling layout and component visibility.
//! - Frame timing.

mod borrowed;
mod components;
mod context;
mod item;
mod live;
mod matches;
mod model;
#[cfg(test)]
pub(crate) mod tests;

use std::num::NonZero;

pub use crate::rect::{Dimensions, Position};
pub use components::Components;
pub use context::Context;
pub use item::{ItemId, ItemRef, OwnedItem};
pub use live::{
    EventBatch, FrameOptions, PreparedFrame, PreparedMatchList, PreparedPreview, PreparedPrompt,
    PreparedStatus, Session,
};
pub use matches::Matches;
pub use model::{Model, ModelOptions};

/// The status on successful session exit.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// The session quit without making a selection.
    Quit,
    /// The session quit and requested a selection. The given item, if any, is the hovered at the time of
    /// processing the exit.
    Selected(Option<ItemId>),
}

/// The queue status, as returned by [`Interface::queue_status`](crate::interface::Interface::queue_status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStatus {
    /// The current number of queued items.
    pub count: u32,
    /// The maximum number of queued items, with `None` meaning that there is no limit.
    pub limit: Option<NonZero<u32>>,
}

/// An event which modifies the list of queued items.
pub enum QueueEvent<'a, T> {
    /// Toggle a single item.
    Toggle(ItemRef<'a, T>),
    /// Remove an item from the queue.
    Remove(ItemId),
    /// A request to queue all currently matching items.
    Matches,
    /// Clear the queue.
    Clear,
}

/// Matching progress after matching updates.
///
/// This is returned by [`Model::update_matches`]. The counts display up-to-date information
/// as available with [`Model::matches`]. Use [`is_complete`](Self::is_complete) for a convenience
/// check that matching is complete.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchStatus {
    /// Whether there were any changes.
    pub items_changed: bool,
    /// The number of items.
    pub matched_item_count: u32,
    /// The total number of match candidates.
    pub item_count: u32,
    /// Whether there is matching work to be done.
    pub matching: bool,
    /// Whether there are any remaining live injectors.
    pub injecting: bool,
}

impl MatchStatus {
    /// Returns if all input has been received and match work has been done.
    ///
    /// This is `!self.matching && !self.injecting`: in other words, all input must be
    /// received and all matching work must be complete against the current query.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.matching && !self.injecting
    }
}
