use std::{
    collections::{
        BTreeMap,
        btree_map::{self, Entry},
    },
    iter::FusedIterator,
};

use nucleo as nc;

use crate::session::{Context, ItemId, ItemRef};

pub(crate) enum QueueEvent<'a, T> {
    /// Toggle a single item.
    Toggle(ItemRef<'a, T>),
    /// Remove an item from the queue.
    Remove(ItemId),
    /// Select all items.
    Matches,
    /// Clear the queue.
    Clear,
}

pub(crate) enum SelectionTarget {
    None,
    Current(ItemId),
    Queued,
}

pub(crate) trait Queued: Sized {
    type Output<'items, T: Send + Sync + 'static>: IntoIterator<Item = &'items T, IntoIter: ExactSizeIterator>;

    #[inline]
    fn is_queued(&self, _id: ItemId) -> bool {
        false
    }

    /// The number of queued items.
    ///
    /// `None` means queing is disabled entirely, whereas `Some(n)` indicates the number of queued
    /// items.
    #[inline]
    fn count(&self) -> Option<usize> {
        None
    }

    /// Select or deselect items.
    ///
    /// The boolean is whether or not there were changes to the queue.
    #[inline]
    fn on_queue<T: Send + Sync + 'static>(
        &mut self,
        _event: QueueEvent<'_, T>,
        _cx: &Context<'_, T>,
    ) -> bool {
        false
    }

    /// Generate the output on completion.
    fn on_select<'items, T: Send + Sync + 'static>(
        self,
        target: SelectionTarget,
        cx: &Context<'items, T>,
    ) -> Self::Output<'items, T>;
}

impl Queued for () {
    type Output<'items, T: Send + Sync + 'static> = Option<&'items T>;

    #[inline]
    fn on_select<'items, T: Send + Sync + 'static>(
        self,
        target: SelectionTarget,
        cx: &Context<'items, T>,
    ) -> Self::Output<'items, T> {
        match target {
            SelectionTarget::None | SelectionTarget::Queued => None,
            SelectionTarget::Current(id) => {
                // SAFETY: the session captures id from this snapshot without ticking or restarting
                // the matcher before output conversion.
                let item = unsafe { cx.item_unchecked(id) };
                Some(item.value())
            }
        }
    }
}

impl Queued for SelectedIndices {
    type Output<'items, T: Send + Sync + 'static> = Selection<'items, T>;

    #[inline]
    fn is_queued(&self, id: ItemId) -> bool {
        self.inner.contains_key(&id)
    }

    #[inline]
    fn count(&self) -> Option<usize> {
        Some(self.inner.len())
    }

    fn on_queue<T: Send + Sync + 'static>(
        &mut self,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        match event {
            QueueEvent::Toggle(item) => {
                let len = self.inner.len();
                match self.inner.entry(item.id()) {
                    Entry::Occupied(entry) => {
                        entry.remove_entry();
                        true
                    }
                    Entry::Vacant(entry) => {
                        if cx
                            .max_selection_count
                            .is_none_or(|l| len < l.get() as usize)
                        {
                            entry.insert(Self::next_order(&mut self.next_order));
                            true
                        } else {
                            false
                        }
                    }
                }
            }
            QueueEvent::Remove(id) => self.inner.remove(&id).is_some(),
            QueueEvent::Matches => {
                let len = self.inner.len();
                let limit = cx
                    .max_selection_count
                    .map_or(usize::MAX, |l| l.get() as usize);
                for id in cx.matches() {
                    if self.inner.len() >= limit {
                        break;
                    }
                    self.insert(id);
                }
                self.inner.len() != len
            }
            QueueEvent::Clear => {
                let changed = !self.inner.is_empty();
                self.inner.clear();
                changed
            }
        }
    }

    #[inline]
    fn on_select<'items, T: Send + Sync + 'static>(
        mut self,
        target: SelectionTarget,
        cx: &Context<'items, T>,
    ) -> Self::Output<'items, T> {
        match target {
            SelectionTarget::None => self.inner.clear(),
            SelectionTarget::Current(id) => {
                self.inner.clear();
                self.insert(id);
            }
            SelectionTarget::Queued => {}
        }
        Selection {
            snapshot: cx.snapshot,
            queued: self,
        }
    }
}

#[derive(Default)]
pub(crate) struct SelectedIndices {
    inner: BTreeMap<ItemId, u64>,
    next_order: u64,
}

impl SelectedIndices {
    fn insert(&mut self, id: ItemId) {
        if let Entry::Vacant(entry) = self.inner.entry(id) {
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
/// This is the selection type of the `pick*` methods of the multi-selection picker variants.
/// Iterate over the picked items with [`iter`](Self::iter) or using `IntoIterator`. Also see the documentation
/// for [multiple selections](crate::Picker#multiple-selections)
///
/// The lifetime of the selection is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    snapshot: &'a nc::Snapshot<T>,
    queued: SelectedIndices,
}

impl<'a, T: Send + Sync + 'static> IntoIterator for Selection<'a, T> {
    type Item = &'a T;
    type IntoIter = SelectionIntoIter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        SelectionIntoIter {
            snapshot: self.snapshot,
            indices: self.queued.inner.into_keys(),
        }
    }
}

/// An owning iterator over a [`Selection`] in item insertion order.
///
/// The order is the same as [`Selection::iter`], except as an owned variant.
pub struct SelectionIntoIter<'a, T: Send + Sync + 'static> {
    snapshot: &'a nc::Snapshot<T>,
    indices: btree_map::IntoKeys<ItemId, u64>,
}

impl<'a, T: Send + Sync + 'static> Iterator for SelectionIntoIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.indices.next().map(|idx| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.snapshot.get_item_unchecked(idx.index).data }
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
        self.indices.nth(n).map(|idx| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.snapshot.get_item_unchecked(idx.index).data }
        })
    }
}

impl<T: Send + Sync + 'static> DoubleEndedIterator for SelectionIntoIter<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices.next_back().map(|idx| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.snapshot.get_item_unchecked(idx.index).data }
        })
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        self.indices.nth_back(n).map(|idx| {
            // SAFETY: Selection owns indices from this snapshot, which remains borrowed.
            unsafe { self.snapshot.get_item_unchecked(idx.index).data }
        })
    }
}

impl<T: Send + Sync + 'static> ExactSizeIterator for SelectionIntoIter<'_, T> {}

impl<T: Send + Sync + 'static> FusedIterator for SelectionIntoIter<'_, T> {}

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
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator {
        self.queued.inner.keys().map(|idx| {
            // SAFETY: the indices were produced by the same snapshot which is stored inside this
            // struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { self.snapshot.get_item_unchecked(idx.index).data }
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
            unsafe { snapshot.get_item_unchecked(idx.index).data }
        })
    }

    /// Returns if there were no selected items.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queued.inner.is_empty()
    }

    /// Returns the number of selected items.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.queued.inner.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    #[test]
    fn selected_indices_track_insertion_order() {
        let mut picker = crate::Picker::new(crate::render::StrRenderer);
        picker.push_batch(["zero", "one", "two", "three"]);
        while picker.core.engine.update(5).matching {}
        let cx = Context::new(&picker.core.engine, None);
        let toggle =
            |index| QueueEvent::Toggle(cx.item(picker.core.engine.item_id(index)).unwrap());
        let mut selection = SelectedIndices::default().on_select(SelectionTarget::Queued, &cx);
        assert!(selection.iter_selected_order().next().is_none());

        assert!(selection.queued.on_queue(toggle(3), &cx));
        assert!(selection.queued.on_queue(toggle(1), &cx));
        assert!(
            selection
                .iter_selected_order()
                .copied()
                .eq(["three", "one"])
        );
        assert!(selection.queued.on_queue(toggle(3), &cx));
        assert!(selection.iter_selected_order().copied().eq(["one"]));
        assert!(selection.queued.on_queue(toggle(3), &cx));
        assert!(
            selection
                .iter_selected_order()
                .copied()
                .eq(["one", "three"])
        );
        assert!(selection.queued.on_queue(toggle(1), &cx));
        assert!(selection.queued.on_queue(toggle(1), &cx));
        assert!(
            selection
                .iter_selected_order()
                .copied()
                .eq(["three", "one"])
        );

        let mut items = selection.into_iter();
        assert_eq!(items.len(), 2);
        assert_eq!(items.next(), Some(&"one"));
        assert_eq!(items.len(), 1);
        assert_eq!(items.next_back(), Some(&"three"));
        assert_eq!(items.size_hint(), (0, Some(0)));
        assert_eq!(items.next(), None);
        assert_eq!(items.next_back(), None);
    }

    #[test]
    fn bulk_queuing_preserves_existing_order_and_obeys_the_context_limit() {
        let mut picker = crate::PickerOptions::new()
            .sort_results(false)
            .reverse_items(true)
            .picker(crate::render::StrRenderer);
        picker.push_batch(["zero", "one", "two", "three"]);
        picker.filter().unwrap();

        let selected = {
            let mut queued = SelectedIndices::default();
            let cx = Context::new(&picker.core.engine, NonZero::new(1));
            let one = cx.item(picker.core.engine.item_id(1)).unwrap();
            assert!(queued.on_queue(QueueEvent::Toggle(one), &cx));
            assert!(!queued.on_queue(QueueEvent::Matches, &cx));

            let cx = Context::new(&picker.core.engine, NonZero::new(3));
            assert!(queued.on_queue(QueueEvent::Matches, &cx));
            assert!(!queued.on_queue(QueueEvent::Matches, &cx));
            assert_eq!(queued.count(), Some(3));
            queued.on_select(SelectionTarget::Queued, &cx)
        };

        assert!(
            selected
                .iter_selected_order()
                .copied()
                .eq(["one", "three", "two"])
        );
        assert!(selected.into_iter().copied().eq(["one", "two", "three"]));
    }
}
