#[cfg(doc)]
use crate::session::Matches;

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
