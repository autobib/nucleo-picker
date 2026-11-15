use std::{
    collections::{
        BTreeMap,
        btree_map::{self, Entry},
    },
    iter::FusedIterator,
};

use super::Multi;
use crate::{
    KeybindDefault,
    event::{Event, keybind_multi},
    session::{Context, Exit, Interface, ItemId, Matches, QueueEvent, QueueStatus},
};
use crossterm::event::KeyEvent;

impl<T: Send + Sync + 'static> Interface<T> for Multi {
    type State = MultiState;
    type Output<'items> = Selection<'items, T>;

    #[inline]
    fn init(&mut self, _cx: &Context<'_, T>) -> Self::State {
        MultiState::default()
    }

    #[inline]
    fn on_restart(&mut self, state: &mut Self::State, _cx: &Context<'_, T>) {
        state.inner.clear();
    }

    #[inline]
    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        state.inner.contains_key(&id)
    }

    #[inline]
    fn queue_status(&self, state: &Self::State) -> Option<QueueStatus> {
        Some(QueueStatus {
            count: u32::try_from(state.inner.len()).unwrap_or(u32::MAX),
            limit: self.max_selection_count,
        })
    }

    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        match event {
            QueueEvent::Toggle(item) => {
                assert!(
                    cx.matches().contains_item(item.id()),
                    "queued item is not in the event context"
                );
                state.assert_generation(item.id());
                let len = state.inner.len();
                match state.inner.entry(item.id()) {
                    Entry::Occupied(entry) => {
                        entry.remove_entry();
                        true
                    }
                    Entry::Vacant(entry) => {
                        if self
                            .max_selection_count
                            .is_none_or(|l| len < l.get() as usize)
                        {
                            entry.insert(MultiState::next_order(&mut state.next_order));
                            true
                        } else {
                            false
                        }
                    }
                }
            }
            QueueEvent::Remove(id) => state.inner.remove(&id).is_some(),
            QueueEvent::Matches => {
                let len = state.inner.len();
                let limit = self
                    .max_selection_count
                    .map_or(usize::MAX, |l| l.get() as usize);
                if len >= limit {
                    return false;
                }
                let matches = cx.matches();
                let Some(id) = matches.ids().next() else {
                    return false;
                };
                state.assert_generation(id);
                for id in matches.ids() {
                    if state.inner.len() >= limit {
                        break;
                    }
                    if let Entry::Vacant(entry) = state.inner.entry(id) {
                        entry.insert(MultiState::next_order(&mut state.next_order));
                    }
                }
                state.inner.len() != len
            }
            QueueEvent::Clear => {
                let changed = !state.inner.is_empty();
                state.inner.clear();
                changed
            }
        }
    }

    #[inline]
    fn finish<'items>(
        &mut self,
        mut state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Quit => state.inner.clear(),
            Exit::Selected(Some(id)) if state.inner.is_empty() => state.insert(id),
            Exit::Selected(_) => {}
        }
        state.into_selection(cx.matches())
    }
}

impl KeybindDefault for Multi {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_multi(key)
    }
}

/// State for the [`Multi`] interface.
///
/// This essentially contains a queue for the selected items.
#[derive(Default)]
pub struct MultiState {
    // A nonempty queue contains only initialized identifiers from a single generation.
    inner: BTreeMap<ItemId, u64>,
    next_order: u64,
}

impl MultiState {
    /// Bind the queued item identifier list with the items in a snapshot.
    ///
    /// Internally, the item queue stores the raw identifiers instead of a handle to the item itself
    /// in order to work more simply around lifetime bounds. This method converts the identifiers
    /// into items using the match list.
    ///
    /// # Panics
    ///
    /// Panics if the queue contains items belonging to a different picker or restart generation.
    #[must_use]
    #[inline]
    pub fn into_selection<'items, T: Send + Sync + 'static>(
        self,
        matches: Matches<'items, T>,
    ) -> Selection<'items, T> {
        assert!(
            self.inner
                .first_key_value()
                .is_none_or(|(&id, _)| matches.contains_item(id)),
            "queued items are not in the output context"
        );
        // SAFETY: the queue contains initialized IDs from one generation, which either
        // matches this snapshot's item storage or has no items to access.
        unsafe { self.into_selection_unchecked(matches) }
    }

    /// Bind the queued item identifier list with the items in a snapshot without checking
    /// provenance.
    ///
    /// # Safety
    ///
    /// The identifiers must correspond to an initialized item in the exact picker and restart
    /// generation corresponding to the provided [`Matches`].
    #[must_use]
    #[inline]
    pub unsafe fn into_selection_unchecked<'items, T: Send + Sync + 'static>(
        self,
        matches: Matches<'items, T>,
    ) -> Selection<'items, T> {
        debug_assert!(
            self.inner
                .first_key_value()
                .is_none_or(|(&id, _)| matches.contains_item(id))
        );
        Selection {
            matches,
            queued: self,
        }
    }

    #[inline]
    fn assert_generation(&self, id: ItemId) {
        assert!(
            self.inner
                .first_key_value()
                .is_none_or(|(&queued, _)| queued.same_generation(id)),
            "cannot queue items from different picker generations"
        );
    }

    #[inline]
    fn insert(&mut self, id: ItemId) {
        self.assert_generation(id);
        if let Entry::Vacant(entry) = self.inner.entry(id) {
            entry.insert(Self::next_order(&mut self.next_order));
        }
    }

    #[inline]
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
/// for [multiple selections](crate::session#multiple-selections)
///
/// The lifetime of the selection is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    matches: Matches<'a, T>,
    queued: MultiState,
}

impl<'a, T: Send + Sync + 'static> IntoIterator for Selection<'a, T> {
    type Item = &'a T;
    type IntoIter = SelectionIntoIter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        SelectionIntoIter {
            matches: self.matches,
            indices: self.queued.inner.into_keys(),
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
        self.queued.inner.keys().map(|&id| {
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
            .inner
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
mod tests;
