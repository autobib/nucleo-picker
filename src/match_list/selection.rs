use std::{
    collections::{BTreeMap, btree_map::Entry},
    num::NonZero,
};

use nucleo as nc;

pub(crate) trait Queued {
    type Output<'a, T: Send + Sync + 'static>;

    fn is_empty(&self) -> bool;

    /// The number of queued items.
    fn len(&self) -> u32;

    fn clear(&mut self) -> bool;

    fn deselect(&mut self, idx: u32) -> bool;

    fn toggle(&mut self, idx: u32) -> bool;

    /// Select a range of items.
    ///
    /// The index is the number of items consumed from the iterator.
    ///
    /// The boolean is whether or not any new items were queued.
    fn select<I: IntoIterator<Item = u32>>(&mut self, items: I) -> (usize, bool);

    fn is_queued(&self, idx: u32) -> bool;

    fn count(&self, limit: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)>;

    fn init(limit: Option<NonZero<u32>>) -> Self;

    fn into_only_selection<T: Send + Sync + 'static>(
        self,
        snapshot: &nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'_, T>;

    fn into_selection<T: Send + Sync + 'static>(
        self,
        snapshot: &nucleo::Snapshot<T>,
    ) -> Self::Output<'_, T>;
}

impl Queued for () {
    type Output<'a, T: Send + Sync + 'static> = Option<&'a T>;

    #[inline]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline]
    fn len(&self) -> u32 {
        0
    }

    #[inline]
    fn clear(&mut self) -> bool {
        false
    }

    #[inline]
    fn deselect(&mut self, _: u32) -> bool {
        false
    }

    #[inline]
    fn toggle(&mut self, _: u32) -> bool {
        false
    }

    #[inline]
    fn select<I: IntoIterator<Item = u32>>(&mut self, _: I) -> (usize, bool) {
        (0, false)
    }

    #[inline]
    fn is_queued(&self, _: u32) -> bool {
        false
    }

    #[inline]
    fn init(_: Option<NonZero<u32>>) -> Self {}

    #[inline]
    fn into_selection<T: Send + Sync + 'static>(
        self,
        _: &nucleo::Snapshot<T>,
    ) -> Self::Output<'_, T> {
        None
    }

    #[inline]
    fn into_only_selection<T: Send + Sync + 'static>(
        self,
        snapshot: &nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'_, T> {
        // SAFETY: the session obtains idx from this snapshot's selected match without updating it.
        Some(unsafe { snapshot.get_item_unchecked(idx).data })
    }

    #[inline]
    fn count(&self, _: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)> {
        None
    }
}

impl Queued for SelectedIndices {
    type Output<'a, T: Send + Sync + 'static> = Selection<'a, T>;

    #[inline]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    #[inline]
    fn clear(&mut self) -> bool {
        if self.is_empty() {
            false
        } else {
            self.inner.clear();
            true
        }
    }

    #[inline]
    fn toggle(&mut self, idx: u32) -> bool {
        let n = self.inner.len();
        match self.inner.entry(idx) {
            Entry::Occupied(occupied_entry) => {
                occupied_entry.remove_entry();
                true
            }
            Entry::Vacant(vacant_entry) => {
                if self.limit.is_none_or(|l| n < l.get() as usize) {
                    vacant_entry.insert(Self::next_order(&mut self.next_order));
                    true
                } else {
                    false
                }
            }
        }
    }

    #[inline]
    fn deselect(&mut self, idx: u32) -> bool {
        self.inner.remove(&idx).is_some()
    }

    fn select<I: IntoIterator<Item = u32>>(&mut self, items: I) -> (usize, bool) {
        let mut toggled = false;
        let mut consumed: usize = 0;

        for it in items {
            let current_len = self.inner.len();
            match self.inner.entry(it) {
                Entry::Vacant(vacant_entry)
                    if self.limit.is_none_or(|l| current_len < l.get() as usize) =>
                {
                    toggled = true;
                    consumed += 1;
                    vacant_entry.insert(Self::next_order(&mut self.next_order));
                }
                Entry::Vacant(_) => break,
                Entry::Occupied(_) => {
                    consumed += 1;
                }
            }
        }

        (consumed, toggled)
    }

    #[inline]
    fn is_queued(&self, idx: u32) -> bool {
        self.inner.contains_key(&idx)
    }

    #[inline]
    fn init(limit: Option<NonZero<u32>>) -> Self {
        Self {
            inner: BTreeMap::new(),
            next_order: 0,
            limit,
        }
    }

    #[inline]
    fn into_selection<T: Send + Sync + 'static>(
        self,
        snapshot: &nucleo::Snapshot<T>,
    ) -> Self::Output<'_, T> {
        Self::Output {
            snapshot,
            queued: self,
        }
    }

    #[inline]
    fn into_only_selection<T: Send + Sync + 'static>(
        mut self,
        snapshot: &nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'_, T> {
        self.insert(idx);
        Self::Output {
            snapshot,
            queued: self,
        }
    }

    #[inline]
    fn count(&self, limit: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)> {
        Some((self.inner.len() as u32, limit))
    }
}

pub(crate) struct SelectedIndices {
    inner: BTreeMap<u32, u64>,
    next_order: u64,
    limit: Option<NonZero<u32>>,
}

impl SelectedIndices {
    fn insert(&mut self, idx: u32) {
        if let Entry::Vacant(entry) = self.inner.entry(idx) {
            entry.insert(Self::next_order(&mut self.next_order));
        }
    }

    fn next_order(next_order: &mut u64) -> u64 {
        let order = *next_order;
        *next_order += 1;
        order
    }
}

/// The selected items when the picker quits.
///
/// This is the return type of the various `pick_multi*` methods of a [`Picker`](crate::Picker).
/// Iterate over the picked items with [`iter`](Self::iter). Also see the documentation for
/// [multiple selections](crate::Picker#multiple-selections)
///
/// The lifetime of this struct is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    snapshot: &'a nc::Snapshot<T>,
    queued: SelectedIndices,
}

impl<'a, T: Send + Sync + 'static> Selection<'a, T> {
    /// Returns an iterator over the other selected items.
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
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator {
        self.queued.inner.keys().map(|idx| {
            // SAFETY: the indices were produced by the same snapshot which is stored inside this
            // struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { self.snapshot.get_item_unchecked(*idx).data }
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
        let snapshot = self.snapshot;
        let mut selected_items = self
            .queued
            .inner
            .iter()
            .map(|(&idx, &order)| (order, idx))
            .collect::<Vec<_>>();
        selected_items.sort_unstable_by_key(|&(order, _)| order);

        selected_items.into_iter().map(move |(_, idx)| {
            // SAFETY: the indices were produced by the same snapshot which is stored inside this
            // struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { snapshot.get_item_unchecked(idx).data }
        })
    }

    /// Returns if there were no selected items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.inner.is_empty()
    }

    /// Returns the number of selected items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.inner.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_indices_track_insertion_order() {
        let mut selected = SelectedIndices::init(None);

        assert!(selected.toggle(3));
        assert!(selected.toggle(1));
        assert!(selected.toggle(3));
        assert!(selected.toggle(3));

        let mut selection_order = selected
            .inner
            .iter()
            .map(|(&idx, &order)| (order, idx))
            .collect::<Vec<_>>();
        selection_order.sort();

        assert_eq!(selection_order, [(1, 1), (2, 3)]);
    }
}
