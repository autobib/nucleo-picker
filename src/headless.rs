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

/// A snapshot of the matching items in a [`Picker`](crate::Picker).
///
/// Items are sorted according to the configured match-list order (that is, it takes into account the [`sort_results`](crate::PickerOptions::sort_results) and [`reverse_items`](crate::PickerOptions::reverse_items) configuration).
///
/// This is a view type only, and is only up-to-date as of the time of the last update.
pub struct Matches<'a, T> {
    pub(crate) snapshot: &'a nucleo::Snapshot<T>,
}

impl<'a, T: Send + Sync + 'static> Matches<'a, T> {
    /// Returns the number of matching items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snapshot.matches().len()
    }

    /// Returns if there are no matching items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshot.matches().is_empty()
    }

    /// Returns the item at the given zero-based match rank, or `None` if it is out of bounds.
    #[must_use]
    pub fn get(&self, rank: usize) -> Option<&'a T> {
        self.snapshot
            .get_matched_item(u32::try_from(rank).ok()?)
            .map(|item| item.data)
    }

    /// Returns an iterator over the matching items in match-list order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator + use<'a, T> {
        self.snapshot.matched_items(..).map(|item| item.data)
    }
}
