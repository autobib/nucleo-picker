//! # Picker session
//!
//! A *session* is a single interactive call to the picker. The behaviour in a session is
//! determined by the [`Interface`](crate::interface::Interface) of the picker.
//!
//! The picker can be re-used for multiple sessions: the items and query are retained between
//! sessions (but can be cleared by restarting). The two main entry points are:
//!
//! - The [`interface`](crate::interface) module documentation.
//! - The [`Session`] struct documentation.
//!
//! ## Session API overview.
//!
//! Here is an overview of using the session API.
//! For an example in code, see the [Ratatui
//! example](https://github.com/autobib/nucleo-picker/blob/master/examples/ratatui.rs).
//!
//! 1. *Initialize the [`Model`].*
//!    The [`Model`] is the core reusable matching state which is independent of the session.
//!    Think of it as the item pool as well as the matching engine.
//!    The most common route is to start with [`ModelOptions`], set configuration as desired, and
//!    then convert it into a [`Model`].
//! 2. *Add items to the [`Model`].*
//!    Using an [injector](Model::injector) or the [convenience blocking API](Model::push_batch),
//!    add items to the model either from a separate thread or inline.
//!    You don't have to add all items now; you can continue to use the injector whenever you wish.
//! 3. *Initialize the [`Session`].*
//!    Do this by calling [`Model::session`].
//! 4. *Execute the frame loop.*
//!    1. *Synchronize matches with the background matching threadpool.*
//!       Do this by calling [`Session::update_matches`].
//!       Matches will not appear without calling this method.
//!    2. *Prepare the frame for rendering.*
//!       Do this by calling [`Session::prepare_frame`].
//!       Preparing the frame gives the [interface](crate::interface::Interface) the chance to do any work which shold occur immediately before the frame is rendering.
//!    3. *Render the frame.*
//!       What this will look like depends on your application.
//!       For a Ratatui application, the various components (accessible on the [`PreparedFrame`])
//!       implement [`Widget`](ratatui_core::widgets::Widget).
//!       You're responsible for managing the screen.
//!       Note that the picker keeps track of components which have logically changed and reports
//!       them using [`PreparedFrame::redraw`], which may be useful to reduce render calls.
//!    4. *Declare rendering to be complete.*
//!       Do this by calling [`PreparedFrame::complete`].
//!       Dropping the prepared frame without calling this method will mean that post-frame
//!       interface hooks will not run.
//!    5. *Propagate events to the picker.*
//!       Begin by getting an [`EventBatch`] with [`Session::events`]
//!       Event handling will depend on the application, but the events which are
//!       relevant for the picker should be forwarded using the various methods on an
//!       [`EventBatch`]. This is how you forward user actions and application actions to the
//!       picker.
//! 5. *Close the session and obtain the selected items.*
//!    When picking is complete, close the session by calling [`Session::finish`]. The passed
//!    [`Exit`] enum indicates if a selection should be made (and if so, what). The interface
//!    subsequently returns the output items for downstream use.
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
//! The caller drives a session to run a picker interface. Generally speaking, the runner is
//! responsible for converting input into events, advancing matching, preparing and drawing frames, handling layout and component visibility, and correctly timing the frame.

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
