use std::{
    collections::{BTreeMap, btree_map::Entry},
    num::NonZero,
};

use nucleo_picker_core::session::{Context, Exit, ItemId, Matches, QueueEvent, QueueStatus};

use crate::{Interface, Selection};

/// The multiple-selection picker interface.
///
/// Initialize using [`Multi::new`] or the [`Default`] implementation.
#[derive(Clone, Copy, Default)]
pub struct Multi {
    max_selection_count: Option<NonZero<u32>>,
}

impl Multi {
    /// Initialize a new multi-selection interface with no bound on the number of selections.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            max_selection_count: None,
        }
    }

    /// The maximum number of items that can be selected in the picker.
    ///
    /// If `None`, no bound is applied; otherwise, use the provided bound. This is a `u32` since
    /// the picker cannot hold more than [`u32::MAX`] items.
    #[must_use]
    #[inline]
    pub const fn max_selection_count(mut self, maximum: Option<NonZero<u32>>) -> Self {
        self.max_selection_count = maximum;
        self
    }
}

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
            queued: self.inner,
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

#[cfg(test)]
mod tests;
