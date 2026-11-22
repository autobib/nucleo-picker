use std::{
    collections::{BTreeMap, btree_map},
    iter::FusedIterator,
};

use nucleo_picker_core::session::{ItemId, Matches};

/// The selected items when the picker quits.
///
/// This is the selection type of the `pick*` methods of the multi-selection picker variants.
/// Iterate over the picked items with [`iter`](Self::iter) or using `IntoIterator`.
///
/// See the documentation for [multiple selections](https://docs.rs/nucleo-picker/latest/nucleo_picker/interface/index.html#multiple-selections)
/// as an example for how the high-level `nucleo-picker` produces this selection.
///
/// The lifetime of the selection is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    pub(crate) matches: Matches<'a, T>,
    pub(crate) queued: BTreeMap<ItemId, u64>,
}

impl<'a, T: Send + Sync + 'static> IntoIterator for Selection<'a, T> {
    type Item = &'a T;
    type IntoIter = SelectionIntoIter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        SelectionIntoIter {
            matches: self.matches,
            indices: self.queued.into_keys(),
        }
    }
}

/// An owning iterator over a [`Selection`] in item insertion order.
///
/// The order is the same as [`Selection::iter`], except as an owned variant.
pub struct SelectionIntoIter<'a, T: Send + Sync + 'static> {
    matches: Matches<'a, T>,
    indices: btree_map::IntoKeys<ItemId, u64>,
}

impl<'a, T: Send + Sync + 'static> Iterator for SelectionIntoIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.indices.next().map(|id| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.matches.item_unchecked(id).value() }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.indices.len()
    }

    #[inline]
    fn last(mut self) -> Option<Self::Item> {
        self.next_back()
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.indices.nth(n).map(|id| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.matches.item_unchecked(id).value() }
        })
    }
}

impl<T: Send + Sync + 'static> DoubleEndedIterator for SelectionIntoIter<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices.next_back().map(|id| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.matches.item_unchecked(id).value() }
        })
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        self.indices.nth_back(n).map(|id| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.matches.item_unchecked(id).value() }
        })
    }
}

impl<T: Send + Sync + 'static> ExactSizeIterator for SelectionIntoIter<'_, T> {}

impl<T: Send + Sync + 'static> FusedIterator for SelectionIntoIter<'_, T> {}

impl<'a, T: Send + Sync + 'static> Selection<'a, T> {
    /// Returns an iterator over the selected items.
    ///
    /// The iterator contains each selected item exactly once, sorted by index based on the order
    /// in which the picker received the items. If multiple threads populate the picker, the
    /// relative order between different threads is unspecified. Note that items are deduplicated
    /// based on the selection index instead of using any properties of the type `T` itself.
    ///
    /// See [`iter_selected_order`](Self::iter_selected_order) to obtain the items in the order
    /// selected by the user.
    ///
    /// The iterator will be empty if the picker quit without selecting any items.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator {
        self.queued.keys().map(|&id| {
            // SAFETY: on creation, the indices were validated to belong to the same snapshot which is stored
            // inside this struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { self.matches.item_unchecked(id).value() }
        })
    }

    /// Returns an iterator over the selected items, sorted by selection order.
    ///
    /// The iterator contains each selected item exactly once. If an item is un-selected and
    /// selected again, the order is determined by the final selection.
    ///
    /// Note that the current implementation does not internally store the selected items by
    /// selection order, so calling this method requires allocating a new container and then sorting.
    ///
    /// The iterator will be empty if the picker quit without selecting any items.
    pub fn iter_selected_order(
        &self,
    ) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator {
        let matches = self.matches;
        let mut selected_items = self
            .queued
            .iter()
            .map(|(&id, &order)| (order, id))
            .collect::<Vec<_>>();
        selected_items.sort_unstable_by_key(|&(order, _)| order);

        selected_items.into_iter().map(move |(_, id)| {
            // SAFETY: on creation, the indices were validated to belong to the same snapshot which is stored
            // inside this struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { matches.item_unchecked(id).value() }
        })
    }

    /// Returns if there were no selected items.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// Returns the number of selected items.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.queued.len()
    }
}
