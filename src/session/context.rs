#[cfg(feature = "preview")]
use super::Dimensions;
use super::{ItemRef, Matches};
use crate::match_engine::MatchEngine;

/// Read-only access to picker context.
// TODO: write more docs; session observation; direct item access through Matches
pub struct Context<'items, T> {
    matches: Matches<'items, T>,
    query: &'items str,
    #[cfg(any(test, feature = "preview"))]
    match_revision: u64,
    rank: Option<u32>,
    #[cfg(feature = "preview")]
    preview_size: Option<Dimensions>,
}

impl<'items, T: Send + Sync + 'static> Context<'items, T> {
    pub(crate) fn new<R>(engine: &'items MatchEngine<T, R>) -> Self {
        Self {
            matches: Matches::new(engine),
            query: engine.query(),
            #[cfg(any(test, feature = "preview"))]
            match_revision: engine.match_revision(),
            rank: None,
            #[cfg(feature = "preview")]
            preview_size: None,
        }
    }

    pub(crate) fn with_rank(mut self, rank: Option<u32>) -> Self {
        self.rank = rank.filter(|&rank| (rank as usize) < self.matches.len());
        self
    }

    #[cfg(feature = "preview")]
    pub(crate) fn with_preview(mut self, size: Option<Dimensions>) -> Self {
        self.preview_size = size;
        self
    }

    /// A token to track the snapshot generation.
    #[cfg(any(test, feature = "preview"))]
    pub(crate) fn match_revision(&self) -> u64 {
        self.match_revision
    }

    /// Returns the hovered item, or none if the match list is empty.
    pub fn hovered(&self) -> Option<ItemRef<'items, T>> {
        self.matches.get(self.rank? as usize)
    }

    /// Returns the rank of the hovered item, or none if the match list is empty.
    pub fn rank(&self) -> Option<u32> {
        self.rank
    }

    /// The current query string.
    pub fn query(&self) -> &str {
        self.query
    }

    /// Returns the dimensions of the preview pane, or `None` if the preview pane is hidden or
    /// otherwise not enabled.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    pub fn preview_size(&self) -> Option<Dimensions> {
        self.preview_size
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
