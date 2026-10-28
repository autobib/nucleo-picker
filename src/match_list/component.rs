use super::{MatchListEvent, MatchListState, QueueEvent, Queued};
use crate::{
    PickerChars,
    component::Component,
    match_engine::MatchEngine,
    rect::{Area, Rect},
    session::{Context, ItemId},
    util::as_u32,
};
use std::io;

pub(crate) struct MatchList<'a, Q> {
    state: &'a mut MatchListState,
    chars: &'a PickerChars,
    queued: Q,
    buffered_selection: u32,
    pending_redraw: bool,
}

impl<'a, Q: Queued> MatchList<'a, Q> {
    pub fn new(state: &'a mut MatchListState, chars: &'a PickerChars, queued: Q) -> Self {
        let buffered_selection = state.layout.selection;
        Self {
            state,
            chars,
            queued,
            buffered_selection,
            pending_redraw: false,
        }
    }

    pub fn queued(&self) -> &Q {
        &self.queued
    }

    pub fn into_queue(self) -> Q {
        self.queued
    }

    pub fn event_selection<T: Send + Sync + 'static, R>(
        &self,
        engine: &MatchEngine<T, R>,
    ) -> Option<u32> {
        (!engine.is_empty()).then_some(self.buffered_selection)
    }

    pub fn selection<T: Send + Sync + 'static, R>(
        &self,
        engine: &MatchEngine<T, R>,
    ) -> Option<u32> {
        self.state.layout.selection(engine.snapshot())
    }

    pub fn restart<T: Send + Sync + 'static>(&mut self, cx: &Context<'_, T>) {
        self.state.layout.restart(cx.snapshot, &self.state.config);
        self.queued.on_queue(QueueEvent::Clear, cx);
        self.buffered_selection = 0;
        self.pending_redraw = true;
    }

    /// Commit buffered navigation movements to the match engine.
    pub fn flush<T: Send + Sync + 'static, R>(&mut self, engine: &MatchEngine<T, R>) {
        self.pending_redraw |= self.state.layout.set_selection(
            engine.snapshot(),
            self.buffered_selection,
            &self.state.config,
        );
    }

    pub fn update<T: Send + Sync + 'static, R>(
        &mut self,
        engine: &MatchEngine<T, R>,
        items_changed: bool,
        query_changed: bool,
    ) -> bool {
        if items_changed {
            self.state
                .layout
                .update_items(engine.snapshot(), &self.state.config);
        }
        self.pending_redraw |= items_changed || query_changed;
        self.buffered_selection = self.state.layout.selection;
        std::mem::take(&mut self.pending_redraw)
    }

    fn toggle_selection<T: Send + Sync + 'static>(&mut self, cx: &Context<'_, T>) -> bool {
        let changed = cx
            .matches()
            .nth(self.buffered_selection as usize)
            .is_some_and(|id| {
                // SAFETY: id comes from this context's borrowed snapshot of matches.
                let item = unsafe { cx.item_unchecked(id) };
                self.queued.on_queue(QueueEvent::Toggle(item), cx)
            });
        self.pending_redraw |= changed;
        changed
    }

    fn queue_range<T: Send + Sync + 'static>(
        &mut self,
        cx: &Context<'_, T>,
        count: usize,
        increasing: bool,
    ) {
        let start = self.buffered_selection as usize;
        let count = count.saturating_add(1);
        let matches = cx.matches().enumerate();
        if increasing {
            self.queue_items(cx, matches.skip(start).take(count));
        } else {
            self.queue_items(cx, matches.take(start + 1).rev().take(count));
        }
    }

    fn queue_items<T: Send + Sync + 'static>(
        &mut self,
        cx: &Context<'_, T>,
        items: impl IntoIterator<Item = (usize, ItemId)>,
    ) {
        for (rank, id) in items {
            if !self.queued.is_queued(id) {
                // SAFETY: queue_range supplies IDs from cx.matches(), whose snapshot stays borrowed.
                let item = unsafe { cx.item_unchecked(id) };
                if !self.queued.on_queue(QueueEvent::Toggle(item), cx) {
                    break;
                }
                self.pending_redraw = true;
            }
            self.buffered_selection = rank as u32;
        }
    }

    fn decr(&mut self, n: usize) {
        self.buffered_selection = self.buffered_selection.saturating_sub(as_u32(n));
    }

    fn incr<T: Send + Sync + 'static>(&mut self, cx: &Context<'_, T>, n: usize) {
        self.buffered_selection = self
            .buffered_selection
            .saturating_add(as_u32(n))
            .min(cx.snapshot.matched_item_count().saturating_sub(1));
    }

    // Handle incoming events: buffer navigation events, but apply queue operations immediately.
    #[inline]
    #[expect(deprecated)]
    pub fn handle<T: Send + Sync + 'static>(&mut self, event: MatchListEvent, cx: &Context<'_, T>) {
        match event {
            MatchListEvent::Up(n) => {
                if self.state.config.reversed {
                    self.decr(n);
                } else {
                    self.incr(cx, n);
                }
            }
            MatchListEvent::ToggleUp(n) => {
                if self.toggle_selection(cx) {
                    if self.state.config.reversed {
                        self.decr(n);
                    } else {
                        self.incr(cx, n);
                    }
                }
            }
            MatchListEvent::Down(n) => {
                if self.state.config.reversed {
                    self.incr(cx, n);
                } else {
                    self.decr(n);
                }
            }
            MatchListEvent::QueueAbove(n) => {
                self.queue_range(cx, n, !self.state.config.reversed);
            }
            MatchListEvent::QueueBelow(n) => {
                self.queue_range(cx, n, self.state.config.reversed);
            }
            MatchListEvent::QueueMatches => {
                self.pending_redraw |= self.queued.on_queue(QueueEvent::Matches, cx);
            }
            MatchListEvent::Unqueue => {
                if let Some(id) = cx.matches().nth(self.buffered_selection as usize) {
                    self.pending_redraw |= self.queued.on_queue(QueueEvent::Remove(id), cx);
                }
            }
            MatchListEvent::UnqueueAll => {
                self.pending_redraw |= self.queued.on_queue(QueueEvent::Clear, cx);
            }
            MatchListEvent::ToggleDown(n) => {
                if self.toggle_selection(cx) {
                    if self.state.config.reversed {
                        self.incr(cx, n);
                    } else {
                        self.decr(n);
                    }
                }
            }
            MatchListEvent::Reset => {
                self.buffered_selection = 0;
            }
        }
    }
}

impl<T: Send + Sync + 'static, R: crate::Render<T>, Q: Queued> Component<MatchEngine<T, R>>
    for MatchList<'_, Q>
{
    fn resize(&mut self, area: Area, engine: &MatchEngine<T, R>) {
        self.state
            .layout
            .resize(engine.snapshot(), area.height, &self.state.config);
    }

    fn draw<D: Rect>(&mut self, engine: &MatchEngine<T, R>, rect: &mut D) -> io::Result<()> {
        self.state.draw_items(engine, rect, self.chars, |idx| {
            self.queued.is_queued(engine.item_id(idx))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Picker,
        match_list::{SelectedIndices, SelectionTarget},
        render::StrRenderer,
    };
    use std::num::NonZero;

    fn picker() -> Picker<&'static str, StrRenderer> {
        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b", "c", "d"]);
        while picker.core.engine.update(5).matching {}
        picker.core.list_state.layout.update_items(
            picker.core.engine.snapshot(),
            &picker.core.list_state.config,
        );
        picker
    }

    fn update(
        list: &mut MatchList<'_, SelectedIndices>,
        engine: &mut MatchEngine<&'static str, StrRenderer>,
    ) -> bool {
        list.flush(engine);
        let status = engine.update(5);
        list.update(engine, status.items_changed, false)
    }

    #[test]
    fn selection_changes_do_not_change_the_queue() {
        let mut picker = picker();
        let mut list = MatchList::new(
            &mut picker.core.list_state,
            &picker.core.chars,
            SelectedIndices::default(),
        );
        list.handle(
            MatchListEvent::Up(1),
            &Context::new(&picker.core.engine, None),
        );
        let redraw = update(&mut list, &mut picker.core.engine);
        assert!(redraw);
        assert_eq!(list.queued().count(), Some(0));
    }

    #[test]
    fn queue_changes_are_reported_separately() {
        let mut picker = picker();
        let mut list = MatchList::new(
            &mut picker.core.list_state,
            &picker.core.chars,
            SelectedIndices::default(),
        );
        list.handle(
            MatchListEvent::ToggleDown(0),
            &Context::new(&picker.core.engine, None),
        );
        assert_eq!(list.queued().count(), Some(1));
        let redraw = update(&mut list, &mut picker.core.engine);
        assert!(redraw);
        assert_eq!(list.selection(&picker.core.engine), Some(0));
        assert!(!update(&mut list, &mut picker.core.engine));
    }

    #[test]
    fn full_queue_blocks_insertion_movement_but_allows_removal_movement() {
        for reversed in [false, true] {
            let mut picker = picker();
            picker.core.list_state.config.reversed = reversed;
            let mut list = MatchList::new(
                &mut picker.core.list_state,
                &picker.core.chars,
                SelectedIndices::default(),
            );
            let toggle = if reversed {
                MatchListEvent::ToggleDown
            } else {
                MatchListEvent::ToggleUp
            };
            let limit = NonZero::new(1);

            list.handle(toggle(1), &Context::new(&picker.core.engine, limit));
            assert_eq!(list.event_selection(&picker.core.engine), Some(1));
            assert_eq!(list.queued().count(), Some(1));
            assert!(update(&mut list, &mut picker.core.engine));

            list.handle(toggle(1), &Context::new(&picker.core.engine, limit));
            assert_eq!(list.event_selection(&picker.core.engine), Some(1));
            assert_eq!(list.queued().count(), Some(1));
            assert!(!update(&mut list, &mut picker.core.engine));

            let cx = Context::new(&picker.core.engine, limit);
            list.handle(MatchListEvent::Reset, &cx);
            list.handle(toggle(2), &cx);
            assert_eq!(list.event_selection(&picker.core.engine), Some(2));
            assert_eq!(list.queued().count(), Some(0));
            assert!(update(&mut list, &mut picker.core.engine));
        }
    }

    #[test]
    #[expect(deprecated)]
    fn deprecated_ranges_traverse_existing_entries_at_capacity() {
        for reversed in [false, true] {
            for increasing in [false, true] {
                let mut picker = picker();
                picker.core.list_state.config.reversed = reversed;
                let cx = Context::new(&picker.core.engine, NonZero::new(2));
                let (start, last) = if increasing { (0, 1) } else { (3, 2) };
                let mut queued = SelectedIndices::default();
                for index in [start, last] {
                    let item = cx.item(picker.core.engine.item_id(index)).unwrap();
                    assert!(queued.on_queue(QueueEvent::Toggle(item), &cx));
                }
                let mut list =
                    MatchList::new(&mut picker.core.list_state, &picker.core.chars, queued);
                let navigate = if reversed {
                    MatchListEvent::Down
                } else {
                    MatchListEvent::Up
                };
                list.handle(navigate(start as usize), &cx);
                let queue = if increasing != reversed {
                    MatchListEvent::QueueAbove
                } else {
                    MatchListEvent::QueueBelow
                };
                list.handle(queue(0), &cx);
                assert_eq!(list.event_selection(&picker.core.engine), Some(start));
                list.handle(queue(usize::MAX), &cx);
                assert_eq!(list.event_selection(&picker.core.engine), Some(last));
                assert_eq!(list.queued().count(), Some(2));
                let selected = list.into_queue().on_select(SelectionTarget::Queued, &cx);
                let expected = if increasing { ["a", "b"] } else { ["d", "c"] };
                assert!(selected.iter_selected_order().copied().eq(expected));
            }
        }
    }

    #[test]
    #[expect(deprecated)]
    fn single_selection_ignores_queue_events() {
        for reversed in [false, true] {
            let mut picker = picker();
            picker.core.list_state.config.reversed = reversed;
            let cx = Context::new(&picker.core.engine, None);
            let mut list = MatchList::new(&mut picker.core.list_state, &picker.core.chars, ());
            let navigate = if reversed {
                MatchListEvent::Down(1)
            } else {
                MatchListEvent::Up(1)
            };
            list.handle(navigate, &cx);
            for event in [
                MatchListEvent::ToggleUp(1),
                MatchListEvent::ToggleDown(1),
                MatchListEvent::QueueAbove(usize::MAX),
                MatchListEvent::QueueBelow(usize::MAX),
                MatchListEvent::QueueMatches,
                MatchListEvent::Unqueue,
                MatchListEvent::UnqueueAll,
            ] {
                list.handle(event, &cx);
                assert_eq!(list.event_selection(&picker.core.engine), Some(1));
                assert_eq!(list.queued().count(), None);
            }
        }
    }
}
