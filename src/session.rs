//! # Picker session
//!
//! A *session* is a single interactive call to the picker. The behaviour in a session is
//! determined by the [`Interface`] of the picker (the type parameter `I` in the definition of the
//! [`Picker`] struct).
//!
//! The picker can be re-used for multiple sessions: the items and query are retained between
//! sessions (but can be cleared by [restarting](Picker::restart)).
//!
//! Read more about interfaces in the [`interface`](crate::interface) module documentation.

mod borrowed;
mod context;
mod driver;
mod item;
mod live;
mod matches;
mod redraw;
mod run;
#[cfg(test)]
pub(crate) mod tests;

use std::num::NonZero;

#[cfg(all(doc, feature = "preview-cache"))]
use crate::interface::CachedPreview;
pub use crate::interface::PickerRef;
#[cfg(doc)]
use crate::interface::{Interface, Multi, Single, UncachedPreview};
pub use crate::rect::Dimensions;
pub use context::Context;
pub use driver::{Driver, DriverOptions};
pub use item::{ItemId, ItemRef, OwnedItem};
pub(crate) use live::{FrameOptions, Geometry, Session};
pub use matches::Matches;
pub use redraw::Redraw;

#[cfg(doc)]
use crate::event::Event;
#[cfg(all(doc, feature = "preview-cache"))]
use crate::preview::Preview;
#[cfg(doc)]
use crate::{Picker, Render, Selection};

/// The status on successful session exit.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// The session quit without making a selection.
    Quit,
    /// The session quit and requested a selection. The given item, if any, is the hovered at the time of
    /// processing the exit.
    Selected(Option<ItemId>),
}

/// The queue status, as returned by [`Interface::queue_status`].
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
/// This is returned by [`Picker::update_matches`](crate::Picker::update_matches). The counts display up-to-date information
/// as available with [`Picker::matches`](crate::Picker::matches). Use [`is_complete`](Self::is_complete) for a convenience
/// check that matching is complete.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchStatus {
    /// Whether there were any changes to the [`Matches`].
    pub items_changed: bool,
    /// The number of items in the [`Matches`]
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
