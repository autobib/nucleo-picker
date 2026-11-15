use std::{
    collections::{
        BTreeMap,
        btree_map::{self, Entry},
    },
    iter::FusedIterator,
    num::NonZero,
};

use super::Multi;
use crate::{
    event::{Event, keybind_multi},
    session::{Context, Exit, Interface, ItemId, Matches, QueueEvent},
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
    fn queued_count(&self, state: &Self::State) -> Option<usize> {
        Some(state.inner.len())
    }

    #[inline]
    fn queue_limit(&self, _state: &Self::State) -> Option<NonZero<u32>> {
        self.max_selection_count
    }

    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        match event {
            QueueEvent::Toggle(item) => {
                assert_eq!(
                    item.id().generation,
                    cx.matches().generation,
                    "queued item is not in the event context"
                );
                state.bind_generation(item.id().generation);
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
                let matches = cx.matches();
                if len >= limit || matches.is_empty() {
                    return false;
                }
                state.bind_generation(matches.generation);
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
    inner: BTreeMap<ItemId, u64>,
    next_order: u64,
    // A nonempty queue contains only initialized identifiers from this generation.
    generation: u32,
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
            self.inner.is_empty() || self.generation == matches.generation,
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
        debug_assert!(self.inner.is_empty() || self.generation == matches.generation);
        Selection {
            snapshot: matches.snapshot,
            queued: self,
        }
    }

    fn bind_generation(&mut self, generation: u32) {
        if self.inner.is_empty() {
            self.generation = generation;
        } else {
            assert_eq!(
                self.generation, generation,
                "cannot queue items from different picker generations"
            );
        }
    }

    fn insert(&mut self, id: ItemId) {
        self.bind_generation(id.generation);
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
/// for [multiple selections](crate::session#multiple-selections)
///
/// The lifetime of the selection is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    snapshot: &'a nucleo::Snapshot<T>,
    queued: MultiState,
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
    snapshot: &'a nucleo::Snapshot<T>,
    indices: btree_map::IntoKeys<ItemId, u64>,
}

// Public hooks can pair state with a different snapshot; validate at lookup without scanning the queue in finish.
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
        self.queued.inner.keys().map(|idx| {
            // SAFETY: on creation, the indices were validated to belong to the same snapshot which is stored
            // inside this struct, and the lifetime prevents the indices from being invalidated until this struct
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
            // SAFETY: on creation, the indices were validated to belong to the same snapshot which is stored
            // inside this struct, and the lifetime prevents the indices from being invalidated until this struct
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
    use crate::{Picker, PickerOptions, Single, render::StrRenderer};
    use std::{
        num::NonZero,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    fn picker(items: impl IntoIterator<Item = &'static str>) -> Picker<&'static str, StrRenderer> {
        let mut picker = PickerOptions::new()
            .threads(NonZero::new(1))
            .sort_results(false)
            .picker(StrRenderer);
        picker.push_batch(items);
        picker.filter().unwrap();
        picker
    }

    #[test]
    fn public_output_hooks_check_stale_and_foreign_identifiers() {
        for stale in [false, true] {
            for index in 0..2 {
                let mut source = picker(["zero", "one"]);
                let other = picker(["other"]);
                let cx = Context::new(&source.core.engine);
                let item = cx.matches().get(index).unwrap();
                let id = item.id();
                let mut multi = Multi::new();
                let mut state = multi.init(&cx);
                assert!(multi.on_queue(&mut state, QueueEvent::Toggle(item), &cx));
                if stale {
                    source.restart();
                    source.push_batch(["replacement"]);
                    source.filter().unwrap();
                }
                let engine = if stale {
                    &source.core.engine
                } else {
                    &other.core.engine
                };
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        Single.finish((), Exit::Selected(Some(id)), Context::new(engine))
                    }))
                    .is_err()
                );
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        multi.finish(
                            MultiState::default(),
                            Exit::Selected(Some(id)),
                            Context::new(engine),
                        )
                    }))
                    .is_err()
                );
                assert!(
                    catch_unwind(AssertUnwindSafe(|| {
                        multi.finish(state, Exit::Selected(None), Context::new(engine))
                    }))
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn queue_rejects_mixed_generations_without_changing_existing_items() {
        let source = picker(["zero", "one"]);
        let other = picker(["other"]);
        let cx = Context::new(&source.core.engine);
        let other_cx = Context::new(&other.core.engine);
        let local = cx.matches().get(0).unwrap();
        let foreign = other_cx.matches().get(0).unwrap();
        for (event, event_cx) in [
            (QueueEvent::Toggle(foreign), &cx),
            (QueueEvent::Toggle(local), &other_cx),
            (QueueEvent::Toggle(foreign), &other_cx),
            (QueueEvent::Matches, &other_cx),
        ] {
            let mut multi = Multi::new();
            let mut state = multi.init(&cx);
            for rank in [1, 0] {
                assert!(multi.on_queue(
                    &mut state,
                    QueueEvent::Toggle(cx.matches().get(rank).unwrap()),
                    &cx,
                ));
            }
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    multi.on_queue(&mut state, event, event_cx)
                }))
                .is_err()
            );
            let selection = state.into_selection(cx.matches());
            assert!(selection.iter().copied().eq(["zero", "one"]));
            assert!(selection.iter_selected_order().copied().eq(["one", "zero"]));
        }
    }

    #[test]
    fn emptied_queues_can_bind_to_another_generation() {
        let source = picker(["original"]);
        let other = picker(["other"]);
        let cx = Context::new(&source.core.engine);
        let other_cx = Context::new(&other.core.engine);
        let item = cx.matches().get(0).unwrap();
        let mut multi = Multi::new();
        for refill in [false, true] {
            for reset in 0..4 {
                let mut state = multi.init(&cx);
                assert!(multi.on_queue(&mut state, QueueEvent::Toggle(item), &cx));
                match reset {
                    0 => assert!(multi.on_queue(&mut state, QueueEvent::Remove(item.id()), &cx)),
                    1 => assert!(multi.on_queue(&mut state, QueueEvent::Toggle(item), &cx)),
                    2 => assert!(multi.on_queue(&mut state, QueueEvent::Clear, &cx)),
                    _ => multi.on_restart(&mut state, &cx),
                }
                if refill {
                    assert!(multi.on_queue(&mut state, QueueEvent::Matches, &other_cx));
                }
                let selection = state.into_selection(other_cx.matches());
                assert!(selection.iter().copied().eq(refill.then_some("other")));
            }
        }
    }

    #[test]
    fn checked_and_unchecked_conversions_preserve_filtered_items_and_order() {
        for unchecked in [false, true] {
            let mut picker = picker(["zero", "one", "two", "three", "four"]);
            let cx = Context::new(&picker.core.engine);
            let mut multi = Multi::new();
            let mut state = multi.init(&cx);
            for rank in [4, 2, 0, 3, 1] {
                assert!(multi.on_queue(
                    &mut state,
                    QueueEvent::Toggle(cx.matches().get(rank).unwrap()),
                    &cx,
                ));
            }
            picker.update_query("absent");
            let matches = picker.filter().unwrap();
            assert!(matches.is_empty());
            let selection = if unchecked {
                // SAFETY: filtering preserves the item storage that supplied every queued ID.
                unsafe { state.into_selection_unchecked(matches) }
            } else {
                state.into_selection(matches)
            };
            assert_eq!(selection.len(), 5);
            assert!(!selection.is_empty());
            assert!(
                selection
                    .iter()
                    .copied()
                    .eq(["zero", "one", "two", "three", "four"])
            );
            assert!(
                selection
                    .iter()
                    .rev()
                    .copied()
                    .eq(["four", "three", "two", "one", "zero"])
            );
            assert!(
                selection
                    .iter_selected_order()
                    .copied()
                    .eq(["four", "two", "zero", "three", "one"])
            );
            let mut items = selection.into_iter();
            assert_eq!(items.nth(1), Some(&"one"));
            assert_eq!(items.len(), 3);
            assert_eq!(items.nth_back(1), Some(&"three"));
            assert_eq!(items.last(), Some(&"two"));
        }
    }

    #[test]
    fn finish_validates_only_items_in_the_effective_output() {
        let source = picker(["original"]);
        let other = picker(["other"]);
        let cx = Context::new(&source.core.engine);
        let foreign = other.matches().get(0).unwrap().id();
        let mut multi = Multi::new();
        for quit in [false, true] {
            let mut state = multi.init(&cx);
            assert!(multi.on_queue(&mut state, QueueEvent::Matches, &cx));
            let (exit, output_cx) = if quit {
                (Exit::Quit, Context::new(&other.core.engine))
            } else {
                (
                    Exit::Selected(Some(foreign)),
                    Context::new(&source.core.engine),
                )
            };
            let selection = multi.finish(state, exit, output_cx);
            assert!(selection.iter().copied().eq((!quit).then_some("original")));
        }
    }

    #[test]
    fn queue_lifecycle_preserves_selection_order_and_obeys_the_limit() {
        let mut picker = crate::PickerOptions::new()
            .sort_results(false)
            .reverse_items(true)
            .picker(crate::render::StrRenderer);
        picker.push_batch(["zero", "one", "two", "three"]);
        picker.filter().unwrap();
        let cx = Context::new(&picker.core.engine);
        let toggle = |index| {
            QueueEvent::Toggle(
                cx.matches()
                    .item(picker.core.engine.item_id(index))
                    .unwrap(),
            )
        };
        let mut interface = Multi::new().max_selection_count(NonZero::new(2));
        let state = interface.init(&cx);
        let mut selection = interface.finish(
            state,
            Exit::Selected(None),
            Context::new(&picker.core.engine),
        );
        assert!(selection.iter_selected_order().next().is_none());

        for (index, expected) in [
            (3, &["three"][..]),
            (1, &["three", "one"]),
            (3, &["one"]),
            (3, &["one", "three"]),
            (1, &["three"]),
            (1, &["three", "one"]),
            (3, &["one"]),
        ] {
            assert!(interface.on_queue(&mut selection.queued, toggle(index), &cx));
            assert!(selection.iter_selected_order().eq(expected));
        }

        interface = interface.max_selection_count(NonZero::new(1));
        assert_eq!(
            Interface::<&str>::queue_limit(&interface, &selection.queued),
            NonZero::new(1)
        );
        assert!(!interface.on_queue(&mut selection.queued, QueueEvent::Matches, &cx));
        interface = interface.max_selection_count(NonZero::new(3));
        assert_eq!(
            Interface::<&str>::queue_limit(&interface, &selection.queued),
            NonZero::new(3)
        );
        assert!(interface.on_queue(&mut selection.queued, QueueEvent::Matches, &cx));
        assert!(!interface.on_queue(&mut selection.queued, QueueEvent::Matches, &cx));
        assert_eq!(
            Interface::<&str>::queued_count(&interface, &selection.queued),
            Some(3)
        );
        assert!(
            selection
                .iter_selected_order()
                .copied()
                .eq(["one", "three", "two"])
        );
        assert!(selection.iter().copied().eq(["one", "two", "three"]));

        let mut items = selection.into_iter();
        assert_eq!(items.len(), 3);
        assert_eq!(items.next(), Some(&"one"));
        assert_eq!(items.len(), 2);
        assert_eq!(items.next_back(), Some(&"three"));
        assert_eq!(items.len(), 1);
        assert_eq!(items.next(), Some(&"two"));
        assert_eq!(items.size_hint(), (0, Some(0)));
        assert_eq!(items.next(), None);
        assert_eq!(items.next_back(), None);
    }
}
