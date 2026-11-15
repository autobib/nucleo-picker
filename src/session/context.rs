use super::{ItemRef, Matches};
use crate::match_engine::MatchEngine;

/// Read-only access to picker context.
// TODO: write more docs; session observation; direct item access through Matches
pub struct Context<'items, T> {
    matches: Matches<'items, T>,
    rank: Option<u32>,
    preview_enabled: bool,
}

impl<'items, T: Send + Sync + 'static> Context<'items, T> {
    pub(crate) fn new<R>(engine: &'items MatchEngine<T, R>) -> Self {
        Self {
            matches: Matches::new(engine),
            rank: None,
            preview_enabled: false,
        }
    }

    pub(crate) fn with_rank(mut self, rank: Option<u32>) -> Self {
        self.rank = rank.filter(|&rank| (rank as usize) < self.matches.len());
        self
    }

    pub(crate) fn with_preview_enabled(mut self, enabled: bool) -> Self {
        self.preview_enabled = enabled;
        self
    }

    /// Returns the hovered item, if any.
    ///
    /// Note that this is `None` if the match list is empty and during initialization (even if the
    /// the match-list is non-emty).
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
    /// The returned [`Matches`] struct has lifetime tied only to the picker and can be used to
    /// access picker items and iterate over the matches in this context.
    #[must_use]
    #[inline]
    pub fn matches(&self) -> Matches<'items, T> {
        self.matches
    }
}
