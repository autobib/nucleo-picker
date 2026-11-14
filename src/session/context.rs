#[cfg(feature = "preview")]
use super::Dimensions;
use super::{ItemId, ItemRef, OwnedItem};
use crate::match_engine::MatchEngine;

/// Read-only access to picker context.
// TODO: write more docs
pub struct Context<'items, T> {
    pub(crate) snapshot: &'items nucleo::Snapshot<T>,
    generation: u32,
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
            snapshot: engine.snapshot(),
            generation: engine.generation(),
            query: engine.query(),
            #[cfg(any(test, feature = "preview"))]
            match_revision: engine.match_revision(),
            rank: None,
            #[cfg(feature = "preview")]
            preview_size: None,
        }
    }

    pub(crate) fn with_rank(mut self, rank: Option<u32>) -> Self {
        self.rank = rank.filter(|&rank| rank < self.snapshot.matched_item_count());
        self
    }

    #[cfg(feature = "preview")]
    pub(crate) fn with_preview(mut self, size: Option<Dimensions>) -> Self {
        self.preview_size = size;
        self
    }

    /// Returns the hovered item, or none if the match list is empty.
    pub fn hovered(&self) -> Option<ItemRef<'items, T>> {
        self.matches().nth(self.rank? as usize)
    }

    /// Returns the rank of the hovered item, or none if the match list is empty.
    pub fn rank(&self) -> Option<u32> {
        self.rank
    }

    /// A token to track the snapshot generation.
    #[cfg(any(test, feature = "preview"))]
    pub(crate) fn match_revision(&self) -> u64 {
        self.match_revision
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

    /// Obtain the item corresponding to the given identifier.
    ///
    /// This returns `None` if the identifier does not correspond to an item in this picker.
    pub fn item(&self, id: ItemId) -> Option<ItemRef<'items, T>> {
        if id.generation != self.generation {
            return None;
        }
        self.snapshot
            .get_item(id.index)
            .map(|item| ItemRef::new(id, item.data))
    }

    /// Obtain the item corresponding to the given identifier without checking that the identifier
    /// is valid.
    ///
    /// This [`ItemId`] must have been witnessed in this exact picker and restart generation. For example, this
    /// is guaranteed to be the case for [`ItemId`]s witnessed in this [`Context`]. If you need a persistent
    /// handle, use [`owned_item`](Self::owned_item).
    ///
    /// # Safety
    ///
    /// The id must correspond to an initialized item in this picker with no restart events
    /// processed.
    #[inline]
    pub unsafe fn item_unchecked(&self, id: ItemId) -> ItemRef<'items, T> {
        debug_assert!(self.item(id).is_some());
        // SAFETY: the caller guarantees that id identifies an initialized item in this snapshot.
        let item = unsafe { self.snapshot.get_item_unchecked(id.index) };
        ItemRef::new(id, item.data)
    }

    /// Iterate over matched items.
    ///
    /// This method does not allocate and the resulting iterator has constant-time `nth` and `nth_back`.
    pub fn matches(
        &self,
    ) -> impl ExactSizeIterator<Item = ItemRef<'items, T>> + DoubleEndedIterator + use<'items, T>
    {
        let snapshot = self.snapshot;
        self.match_ids().map(move |id| {
            // SAFETY: id comes from this context's borrowed snapshot of matches.
            let item = unsafe { snapshot.get_item_unchecked(id.index) };
            ItemRef::new(id, item.data)
        })
    }

    /// Iterate over the item identifiers in match order.
    ///
    /// This method does not allocate and the resulting iterator has constant-time `nth` and `nth_back`.
    pub fn match_ids(
        &self,
    ) -> impl ExactSizeIterator<Item = ItemId> + DoubleEndedIterator + use<'items, T> {
        let generation = self.generation;
        self.snapshot.matches().iter().map(move |m| ItemId {
            generation,
            index: m.idx,
        })
    }

    /// Get a persistent handle to an item in the picker.
    ///
    /// This is the same as [`item`](Self::item), except the resulting item has lifetime independent
    /// of the `'items` lifetime. On the other hand, this has slightly more overhead (currently an atomic
    /// reference increment).
    ///
    /// Read the documentation for [`OwnedItem`] for more details.
    pub fn owned_item(&self, id: ItemId) -> Option<OwnedItem<T>> {
        if id.generation != self.generation {
            return None;
        }
        self.snapshot
            .get_detached_item(id.index)
            .map(|item| OwnedItem::new(id, item))
    }
}
