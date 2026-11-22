use super::{ItemId, ItemRef, OwnedItem};
use crate::{Generation, match_engine::MatchEngine};

/// A snapshot of the matching items in a [`Model`](crate::session::Model).
///
/// Items are sorted according to the configured match-list order (that is, it takes into account the [`sort_results`](crate::session::ModelOptions::sort_results) and [`reverse_items`](crate::session::ModelOptions::reverse_items) configuration).
///
/// This is a view type only, and is only up-to-date as of the time of the last update.
pub struct Matches<'items, T> {
    pub(crate) snapshot: &'items nucleo::Snapshot<T>,
    pub(crate) generation: Generation,
}

impl<T> Copy for Matches<'_, T> {}

impl<T> Clone for Matches<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'items, T: Send + Sync + 'static> Matches<'items, T> {
    pub(crate) fn new<R>(engine: &'items MatchEngine<T, R>) -> Self {
        Self {
            snapshot: engine.snapshot(),
            generation: engine.generation(),
        }
    }

    /// Returns the number of matching items.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.snapshot.matches().len()
    }

    /// Returns if there are no matching items.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.snapshot.matches().is_empty()
    }

    /// Returns the item at the given zero-based match rank, or `None` if it is out of bounds.
    #[must_use]
    #[inline]
    pub fn get(&self, rank: usize) -> Option<ItemRef<'items, T>> {
        self.iter().nth(rank)
    }

    /// Returns an iterator over the matching items in match-list order.
    ///
    /// This method does not allocate and the resulting iterator has constant-time `nth` and `nth_back`. The item itself can be obtained with [`ItemRef::value`].
    #[inline]
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = ItemRef<'items, T>> + DoubleEndedIterator + use<'items, T>
    {
        let snapshot = self.snapshot;
        self.ids().map(move |id| {
            // SAFETY: id comes from this context's borrowed snapshot of matches.
            let item = unsafe { snapshot.get_item_unchecked(id.index) };
            ItemRef::new(id, item.data)
        })
    }

    /// Iterate over the item identifiers in match order.
    ///
    /// This method does not allocate and the resulting iterator has constant-time `nth` and `nth_back`.
    #[inline]
    pub fn ids(
        &self,
    ) -> impl ExactSizeIterator<Item = ItemId> + DoubleEndedIterator + use<'items, T> {
        let generation = self.generation;
        self.snapshot.matches().iter().map(move |m| ItemId {
            generation,
            index: m.idx,
        })
    }

    /// Returns the current generation.
    ///
    /// See the [`Generation`] docs for more detail.
    #[must_use]
    #[inline]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Obtain the item corresponding to the given identifier.
    ///
    /// The identifiers must correspond to a valid index in this exact picker and
    /// [generation](Self::generation).
    /// Otherwise, this will return [`None`].
    #[inline]
    pub fn item(&self, id: ItemId) -> Option<ItemRef<'items, T>> {
        if id.generation() != self.generation() {
            return None;
        }
        // SAFETY: identifiers are issued only for initialized items. Equal globally unique
        // generations identify the same append-only item storage, retained by this snapshot.
        Some(unsafe { self.item_unchecked(id) })
    }

    /// Obtain the item corresponding to the given identifier without checking that the identifier
    /// is valid.
    ///
    /// This [`ItemId`] must have been witnessed in this exact picker and restart generation. For example, this
    /// is guaranteed to be the case for [`ItemId`]s witnessed in the [`Matches`] here.
    /// If you need a persistent handle, use [`owned_item`](Self::owned_item).
    ///
    /// # Safety
    ///
    /// The id must correspond to an initialized item in this picker with no restart events
    /// processed in the interim. Equivalently, the [generation](Generation) of the item and this
    /// struct must be the same.
    #[inline]
    pub unsafe fn item_unchecked(&self, id: ItemId) -> ItemRef<'items, T> {
        debug_assert_eq!(id.generation(), self.generation());
        debug_assert!(self.snapshot.get_item(id.index).is_some());
        // SAFETY: the caller guarantees that id identifies an initialized item in this snapshot.
        let item = unsafe { self.snapshot.get_item_unchecked(id.index) };
        ItemRef::new(id, item.data)
    }

    /// Get a persistent handle to an item in the picker.
    ///
    /// This is the same as [`item`](Self::item), except the resulting item has lifetime independent
    /// of the `'items` lifetime. On the other hand, this has slightly more overhead (currently an atomic
    /// reference increment).
    ///
    /// Read the documentation for [`OwnedItem`] for more details.
    #[inline]
    pub fn owned_item(&self, id: ItemId) -> Option<OwnedItem<T>> {
        if id.generation() != self.generation() {
            return None;
        }
        // SAFETY: issued identifiers identify initialized items in this generation's storage.
        Some(unsafe { self.owned_item_unchecked(id) })
    }

    /// Get a persistent handle to an item in the picker without checking validity.
    ///
    /// # Safety
    ///
    /// The id must correspond to an initialized item in this picker with no restart events
    /// processed in the interim. Equivalently, the [generation](Generation) of this struct and of
    /// the item id must be the same.
    #[inline]
    pub unsafe fn owned_item_unchecked(&self, id: ItemId) -> OwnedItem<T> {
        debug_assert_eq!(id.generation(), self.generation());
        debug_assert!(self.snapshot.get_item(id.index).is_some());
        // SAFETY: the caller guarantees that id identifies an initialized item in this snapshot.
        let item = unsafe { self.snapshot.get_detached_item_unchecked(id.index) };
        OwnedItem::new(id, item)
    }
}
