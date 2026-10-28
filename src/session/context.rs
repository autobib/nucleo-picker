use std::num::NonZero;

use super::{ItemId, ItemRef};
use crate::match_engine::MatchEngine;

pub(crate) struct Context<'items, T> {
    pub snapshot: &'items nucleo::Snapshot<T>,
    pub max_selection_count: Option<NonZero<u32>>,
    generation: u32,
}

impl<'items, T: Send + Sync + 'static> Context<'items, T> {
    pub fn new<R>(
        engine: &'items MatchEngine<T, R>,
        max_selection_count: Option<NonZero<u32>>,
    ) -> Self {
        Self {
            snapshot: engine.snapshot(),
            max_selection_count,
            generation: engine.generation(),
        }
    }

    pub fn item(&self, id: ItemId) -> Option<ItemRef<'items, T>> {
        if id.generation != self.generation {
            return None;
        }
        self.snapshot
            .get_item(id.index)
            .map(|item| ItemRef::new(id, item.data))
    }

    /// # Safety
    ///
    /// The id must correspond to an initialized item in this exact picer.
    #[inline]
    pub unsafe fn item_unchecked(&self, id: ItemId) -> ItemRef<'items, T> {
        debug_assert!(self.item(id).is_some());
        // SAFETY: the caller guarantees that id identifies an initialized item in this snapshot.
        let item = unsafe { self.snapshot.get_item_unchecked(id.index) };
        ItemRef::new(id, item.data)
    }

    pub fn matches(
        &self,
    ) -> impl ExactSizeIterator<Item = ItemId> + DoubleEndedIterator + use<'items, T> {
        let generation = self.generation;
        self.snapshot.matches().iter().map(move |m| ItemId {
            generation,
            index: m.idx,
        })
    }
}
