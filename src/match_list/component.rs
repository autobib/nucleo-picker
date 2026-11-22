use super::{MatchListEvent, MatchListState};
use crate::{
    PickerChars,
    component::Component,
    interface::Interface,
    match_engine::MatchEngine,
    rect::{Area, Rect},
    session::{Context, ItemId, QueueEvent},
    util::as_u32,
};
use std::io;

pub(crate) struct MatchList<'a> {
    state: &'a mut MatchListState,
    chars: &'a PickerChars,
    buffered_rank: u32,
    pending_redraw: bool,
}

impl<'a> MatchList<'a> {
    pub fn new(state: &'a mut MatchListState, chars: &'a PickerChars) -> Self {
        let buffered_rank = state.layout.rank;
        Self {
            state,
            chars,
            buffered_rank,
            pending_redraw: false,
        }
    }

    pub fn event_rank<T: Send + Sync + 'static, R>(
        &self,
        engine: &MatchEngine<T, R>,
    ) -> Option<u32> {
        (!engine.is_empty()).then_some(self.buffered_rank)
    }

    pub fn rank<T: Send + Sync + 'static, R>(&self, engine: &MatchEngine<T, R>) -> Option<u32> {
        self.state.layout.rank(engine.snapshot())
    }

    pub fn restart<T: Send + Sync + 'static>(&mut self, cx: &Context<'_, T>) {
        self.state
            .layout
            .restart(cx.matches().snapshot, &self.state.config);
        self.buffered_rank = 0;
        self.pending_redraw = true;
    }

    /// Commit buffered navigation movements to the match engine.
    pub fn flush<T: Send + Sync + 'static, R>(&mut self, engine: &MatchEngine<T, R>) {
        self.pending_redraw |=
            self.state
                .layout
                .set_rank(engine.snapshot(), self.buffered_rank, &self.state.config);
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
        self.buffered_rank = self.state.layout.rank;
        std::mem::take(&mut self.pending_redraw)
    }

    fn toggle_hovered<T: Send + Sync + 'static, I: Interface<T>>(
        &mut self,
        interface: &mut I,
        state: &mut I::State,
        cx: &Context<'_, T>,
    ) -> bool {
        let changed = cx
            .matches()
            .get(self.buffered_rank as usize)
            .is_some_and(|item| interface.on_queue(state, QueueEvent::Toggle(item), cx));
        self.pending_redraw |= changed;
        changed
    }

    fn queue_range<T: Send + Sync + 'static, I: Interface<T>>(
        &mut self,
        interface: &mut I,
        state: &mut I::State,
        cx: &Context<'_, T>,
        count: usize,
        increasing: bool,
    ) {
        let start = self.buffered_rank as usize;
        let count = count.saturating_add(1);
        let matches = cx.matches().ids().enumerate();
        if increasing {
            self.queue_items(interface, state, cx, matches.skip(start).take(count));
        } else {
            self.queue_items(
                interface,
                state,
                cx,
                matches.take(start + 1).rev().take(count),
            );
        }
    }

    fn queue_items<T: Send + Sync + 'static, I: Interface<T>>(
        &mut self,
        interface: &mut I,
        state: &mut I::State,
        cx: &Context<'_, T>,
        items: impl IntoIterator<Item = (usize, ItemId)>,
    ) {
        for (rank, id) in items {
            if !interface.is_queued(state, id) {
                // SAFETY: queue_range supplies IDs from cx.matches().ids(), whose snapshot stays borrowed.
                let item = unsafe { cx.matches().item_unchecked(id) };
                if !interface.on_queue(state, QueueEvent::Toggle(item), cx) {
                    break;
                }
                self.pending_redraw = true;
            }
            self.buffered_rank = rank as u32;
        }
    }

    fn decr(&mut self, n: usize) {
        self.buffered_rank = self.buffered_rank.saturating_sub(as_u32(n));
    }

    fn incr<T: Send + Sync + 'static>(&mut self, cx: &Context<'_, T>, n: usize) {
        self.buffered_rank = self
            .buffered_rank
            .saturating_add(as_u32(n))
            .min(as_u32(cx.matches().len()).saturating_sub(1));
    }

    // Handle incoming events: buffer navigation events, but apply queue operations immediately.
    #[inline]
    #[expect(deprecated)]
    pub fn handle<T: Send + Sync + 'static, I: Interface<T>>(
        &mut self,
        event: MatchListEvent,
        interface: &mut I,
        state: &mut I::State,
        cx: &Context<'_, T>,
    ) {
        match event {
            MatchListEvent::Up(n) => {
                if self.state.config.reversed {
                    self.decr(n);
                } else {
                    self.incr(cx, n);
                }
            }
            MatchListEvent::ToggleUp(n) => {
                if self.toggle_hovered(interface, state, cx) {
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
                self.queue_range(interface, state, cx, n, !self.state.config.reversed);
            }
            MatchListEvent::QueueBelow(n) => {
                self.queue_range(interface, state, cx, n, self.state.config.reversed);
            }
            MatchListEvent::QueueMatches => {
                self.pending_redraw |= interface.on_queue(state, QueueEvent::Matches, cx);
            }
            MatchListEvent::Unqueue => {
                if let Some(id) = cx.matches().ids().nth(self.buffered_rank as usize) {
                    self.pending_redraw |= interface.on_queue(state, QueueEvent::Remove(id), cx);
                }
            }
            MatchListEvent::UnqueueAll => {
                self.pending_redraw |= interface.on_queue(state, QueueEvent::Clear, cx);
            }
            MatchListEvent::ToggleDown(n) => {
                if self.toggle_hovered(interface, state, cx) {
                    if self.state.config.reversed {
                        self.incr(cx, n);
                    } else {
                        self.decr(n);
                    }
                }
            }
            MatchListEvent::Reset => {
                self.buffered_rank = 0;
            }
        }
    }
}

pub(crate) struct MatchListView<'a, 'session, T, R, I: Interface<T>> {
    pub list: &'a mut MatchList<'session>,
    pub engine: &'a MatchEngine<T, R>,
    pub interface: &'a I,
    pub state: &'a I::State,
}

impl<T: Send + Sync + 'static, R: crate::Render<T>, I: Interface<T>> Component
    for MatchListView<'_, '_, T, R, I>
{
    fn resize(&mut self, area: Area) {
        self.list
            .state
            .layout
            .resize(self.engine.snapshot(), area.height, &self.list.state.config);
    }

    fn draw<D: Rect>(&mut self, rect: &mut D) -> io::Result<()> {
        self.list
            .state
            .draw_items(self.engine, rect, self.list.chars, |idx| {
                self.interface
                    .is_queued(self.state, self.engine.item_id(idx))
            })
    }
}
