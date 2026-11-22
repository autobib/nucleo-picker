use super::{Components, Context, Exit, ItemId, QueueEvent, QueueStatus};
use crate::event::PreviewEvent;
use crate::interface::Interface;
use crate::preview::Content;
use crate::rect::Dimensions;
use std::time::Instant;

impl<T, I: Interface<T>> Interface<T> for &mut I {
    type State = I::State;
    type Output<'a>
        = I::Output<'a>
    where
        T: 'a;

    const PREVIEW: bool = I::PREVIEW;

    fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
        (**self).init(cx)
    }

    fn on_restart(&mut self, state: &mut Self::State, cx: &Context<'_, T>) {
        (**self).on_restart(state, cx);
    }

    fn on_end(&mut self, state: &mut Self::State) {
        (**self).on_end(state);
    }

    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        (**self).is_queued(state, id)
    }

    fn queue_status(&self, state: &Self::State) -> Option<QueueStatus> {
        (**self).queue_status(state)
    }

    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        (**self).on_queue(state, event, cx)
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        (**self).finish(state, exit, cx)
    }

    fn before_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        matches_changed: bool,
        deadline: Instant,
    ) -> Components {
        (**self).before_frame(state, cx, matches_changed, deadline)
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) -> Components {
        (**self).after_frame(state, cx, deadline)
    }

    fn preview_content<'a>(&'a self, state: &'a Self::State) -> Content<'a> {
        (**self).preview_content(state)
    }

    fn preview_event(
        &mut self,
        state: &mut Self::State,
        event: PreviewEvent,
        cx: &Context<'_, T>,
    ) -> Components {
        (**self).preview_event(state, event, cx)
    }

    fn preview_resize(&mut self, state: &mut Self::State, size: Option<Dimensions>) {
        (**self).preview_resize(state, size);
    }
}
