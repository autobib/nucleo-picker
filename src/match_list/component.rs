use super::{MatchListEvent, MatchListState, Queued};
use crate::{
    PickerChars,
    component::Component,
    match_engine::MatchEngine,
    rect::{Area, Rect},
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

    pub fn restart<T: Send + Sync + 'static, R>(&mut self, engine: &MatchEngine<T, R>) {
        self.state
            .layout
            .restart(engine.snapshot(), &self.state.config);
        self.queued.clear();
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

    fn toggle_selection<T: Send + Sync + 'static, R>(
        &mut self,
        engine: &MatchEngine<T, R>,
    ) -> bool {
        if !engine.is_empty()
            && self
                .queued
                .toggle(engine.idx_from_match(self.buffered_selection))
        {
            self.pending_redraw = true;
            true
        } else {
            false
        }
    }

    fn queue_items_above<T: Send + Sync + 'static, R>(
        &mut self,
        engine: &MatchEngine<T, R>,
        count: usize,
    ) -> (usize, bool) {
        let matches = engine.snapshot().matches();
        let start = self.buffered_selection as usize;
        let end = start.saturating_add(count).min(matches.len() - 1);
        self.queued
            .select(matches[start..=end].iter().map(|m| m.idx))
    }

    fn queue_items_below<T: Send + Sync + 'static, R>(
        &mut self,
        engine: &MatchEngine<T, R>,
        count: usize,
    ) -> (usize, bool) {
        let matches = engine.snapshot().matches();
        let start = self.buffered_selection as usize;
        let end = start.saturating_sub(count);
        self.queued
            .select(matches[end..=start].iter().rev().map(|m| m.idx))
    }

    fn decr(&mut self, n: usize) {
        self.buffered_selection = self.buffered_selection.saturating_sub(as_u32(n));
    }

    fn incr<T: Send + Sync + 'static, R>(&mut self, engine: &MatchEngine<T, R>, n: usize) {
        self.buffered_selection = self
            .buffered_selection
            .saturating_add(as_u32(n))
            .min(engine.snapshot().matched_item_count().saturating_sub(1));
    }

    // Handle incoming events: buffer navigation events, but apply queue operations immediately.
    #[inline]
    pub fn handle<T: Send + Sync + 'static, R>(
        &mut self,
        event: MatchListEvent,
        engine: &MatchEngine<T, R>,
    ) {
        match event {
            MatchListEvent::Up(n) => {
                if self.state.config.reversed {
                    self.decr(n);
                } else {
                    self.incr(engine, n);
                }
            }
            MatchListEvent::ToggleUp(n) => {
                if self.toggle_selection(engine) {
                    if self.state.config.reversed {
                        self.decr(n);
                    } else {
                        self.incr(engine, n);
                    }
                }
            }
            MatchListEvent::Down(n) => {
                if self.state.config.reversed {
                    self.incr(engine, n);
                } else {
                    self.decr(n);
                }
            }
            MatchListEvent::QueueAbove(n) => {
                if !engine.is_empty() {
                    if self.state.config.reversed {
                        let (shift, toggled) = self.queue_items_below(engine, n);
                        self.pending_redraw |= toggled;
                        self.decr(shift.saturating_sub(1));
                    } else {
                        let (shift, toggled) = self.queue_items_above(engine, n);
                        self.pending_redraw |= toggled;
                        self.incr(engine, shift.saturating_sub(1));
                    }
                }
            }
            MatchListEvent::QueueBelow(n) => {
                if !engine.is_empty() {
                    if self.state.config.reversed {
                        let (shift, toggled) = self.queue_items_above(engine, n);
                        self.pending_redraw |= toggled;
                        self.incr(engine, shift.saturating_sub(1));
                    } else {
                        let (shift, toggled) = self.queue_items_below(engine, n);
                        self.pending_redraw |= toggled;
                        self.decr(shift.saturating_sub(1));
                    }
                }
            }
            MatchListEvent::QueueMatches => {
                self.pending_redraw |= self
                    .queued
                    .select(engine.snapshot().matches().iter().map(|m| m.idx))
                    .1;
            }
            MatchListEvent::Unqueue => {
                self.pending_redraw |= !engine.is_empty()
                    && self
                        .queued
                        .deselect(engine.idx_from_match(self.buffered_selection));
            }
            MatchListEvent::UnqueueAll => {
                self.pending_redraw |= self.queued.clear();
            }
            MatchListEvent::ToggleDown(n) => {
                if self.toggle_selection(engine) {
                    if self.state.config.reversed {
                        self.incr(engine, n);
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
        self.state
            .draw_items(engine, rect, self.chars, |idx| self.queued.is_queued(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Picker, match_list::SelectedIndices, render::StrRenderer};

    fn picker() -> Picker<&'static str, StrRenderer> {
        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b"]);
        while picker.engine.update(5).matching {}
        picker
            .list_state
            .layout
            .update_items(picker.engine.snapshot(), &picker.list_state.config);
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
            &mut picker.list_state,
            &picker.chars,
            SelectedIndices::init(None),
        );
        list.handle(MatchListEvent::Up(1), &picker.engine);
        let redraw = update(&mut list, &mut picker.engine);
        assert!(redraw);
        assert!(list.queued().is_empty());
    }

    #[test]
    fn queue_changes_are_reported_separately() {
        let mut picker = picker();
        let mut list = MatchList::new(
            &mut picker.list_state,
            &picker.chars,
            SelectedIndices::init(None),
        );
        list.handle(MatchListEvent::ToggleDown(0), &picker.engine);
        assert_eq!(list.queued().len(), 1);
        let redraw = update(&mut list, &mut picker.engine);
        assert!(redraw);
        assert_eq!(list.selection(&picker.engine), Some(0));
        assert!(!update(&mut list, &mut picker.engine));
    }
}
