use super::{ItemRef, Matches};

/// Read-only access to match engine context.
// TODO: write more docs; session observation; direct item access through Matches
pub struct Context<'items, T> {
    matches: Matches<'items, T>,
    rank: Option<u32>,
    preview_enabled: bool,
}

impl<'items, T: Send + Sync + 'static> Context<'items, T> {
    /// Initialize from a snapshot of [`Matches`] from a picker.
    ///
    /// The rank is initially set to `None` and the preview pane is recorded as disabled. Set these
    /// values using [`with_rank`](Self::with_rank) and
    /// [`with_preview_enabled`](Self::with_preview_enabled).
    #[must_use]
    #[inline]
    pub(crate) fn new(matches: Matches<'items, T>) -> Self {
        Self {
            matches,
            rank: None,
            preview_enabled: false,
        }
    }

    /// Set the rank to a given index.
    #[must_use]
    #[inline]
    pub(crate) fn with_rank(mut self, rank: Option<u32>) -> Self {
        self.rank = rank.filter(|&rank| (rank as usize) < self.matches.len());
        self
    }

    #[must_use]
    #[inline]
    pub(crate) fn with_preview_enabled(mut self, enabled: bool) -> Self {
        self.preview_enabled = enabled;
        self
    }

    /// Returns the hovered item, or `None` if the match list is empty.
    pub fn hovered(&self) -> Option<ItemRef<'items, T>> {
        self.matches.get(self.rank? as usize)
    }

    /// Returns the rank of the hovered item, or none if the match list is empty.
    pub fn rank(&self) -> Option<u32> {
        self.rank
    }

    /// Returns whether the preview pane is enabled.
    ///
    /// For example, this method returns `false` if the user has requested that the preview pane is
    /// hidden. Implementations may use this information to skip background preview preparation.
    pub fn preview_enabled(&self) -> bool {
        self.preview_enabled
    }

    /// Return a view of the currently matching items in the picker.
    ///
    /// The returned [`Matches`] struct has lifetime tied to the underlying item storage and can be used to
    /// access picker items and iterate over the matches in this context.
    #[must_use]
    #[inline]
    pub fn matches(&self) -> Matches<'items, T> {
        self.matches
    }
}
