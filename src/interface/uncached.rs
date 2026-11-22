use std::time::Instant;

use crossterm::event::KeyEvent;

use super::{Interface, Multi, Single};
use crate::{
    event::{Event, KeybindDefault},
    keybind::keybind_passthrough_preview,
    preview::{Content, Pane, PreviewEvent},
    session::{Context, Dimensions, Exit, ItemId, QueueEvent, QueueStatus, Redraw},
};

/// A minimal preview adapter holding a closure which generates previews directly.
///
/// In general, you should prefer to use [`CachedPreview`](crate::interface::CachedPreview). This adapter only
/// holds a single pane buffer internally and calls the closure synchronously on the picker thread
/// every time the item changes or no refresh. In particular, the closure must not block. This is
/// sufficient only for small previews built directly from data accessible from the item `&T`.
///
/// For slow previews, use [`CachedPreview`](crate::interface::CachedPreview) with the built-in [`PoolPreviewer`](crate::preview::PoolPreviewer), or implement a custom [`Preview`](crate::preview::Preview) directly.
pub struct UncachedPreview<I, F> {
    interface: I,
    previewer: F,
}

/// Opaque per-session state for [`UncachedPreview`].
///
/// This contains the interface state for the base interface along with resuable pane scratch space.
pub struct UncachedPreviewState<S> {
    interface: S,
    pane: Pane,
    displayed: Option<ItemId>,
    refresh: bool,
    size: Dimensions,
}

impl<I, F> UncachedPreview<I, F> {
    /// Initialize by wrapping an internal interface.
    #[must_use]
    #[inline]
    pub const fn new(interface: I, previewer: F) -> Self {
        Self {
            interface,
            previewer,
        }
    }

    /// Get a reference to the inner interface.
    #[must_use]
    #[inline]
    pub const fn inner(&self) -> &I {
        &self.interface
    }

    /// Get a mutable reference to the inner interface.
    #[must_use]
    #[inline]
    pub const fn inner_mut(&mut self) -> &mut I {
        &mut self.interface
    }
}

impl<F> UncachedPreview<Single, F> {
    /// A convenience method to initialize the interface in single-selection mode.
    #[must_use]
    #[inline]
    pub const fn new_single(previewer: F) -> Self {
        Self::new(Single, previewer)
    }
}

impl<F> UncachedPreview<Multi, F> {
    /// A convenience method to initialize the interface in multi-selection mode.
    #[must_use]
    #[inline]
    pub const fn new_multi(previewer: F) -> Self {
        Self::new(Multi::new(), previewer)
    }
}

impl<T, I, F> Interface<T> for UncachedPreview<I, F>
where
    T: Send + Sync + 'static,
    I: Interface<T>,
    F: FnMut(&T, &mut Pane),
{
    type State = UncachedPreviewState<I::State>;
    type Output<'items> = I::Output<'items>;

    const PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
        UncachedPreviewState {
            interface: self.interface.init(cx),
            pane: Pane::new(),
            displayed: None,
            refresh: false,
            size: Dimensions::default(),
        }
    }

    fn on_restart(&mut self, state: &mut Self::State, cx: &Context<'_, T>) {
        self.interface.on_restart(&mut state.interface, cx);
        state.displayed = None;
        state.refresh = false;
    }

    fn on_end(&mut self, state: &mut Self::State) {
        self.interface.on_end(&mut state.interface);
    }

    #[inline]
    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        self.interface.is_queued(&state.interface, id)
    }

    #[inline]
    fn queue_status(&self, state: &Self::State) -> Option<QueueStatus> {
        self.interface.queue_status(&state.interface)
    }

    #[inline]
    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        self.interface.on_queue(&mut state.interface, event, cx)
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        self.interface.finish(state.interface, exit, cx)
    }

    fn before_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        matches_changed: bool,
        deadline: Instant,
    ) -> Redraw {
        let mut redraw =
            self.interface
                .before_frame(&mut state.interface, cx, matches_changed, deadline);
        if !cx.preview_enabled() {
            return redraw;
        }
        let Some(item) = cx.hovered() else {
            if state.displayed.take().is_some() {
                redraw |= Redraw::PREVIEW;
            }
            return redraw;
        };
        if state.displayed != Some(item.id()) || state.refresh {
            state.pane.clear();
            (self.previewer)(item.value(), &mut state.pane);
            state.displayed = Some(item.id());
            state.refresh = false;
            redraw |= Redraw::PREVIEW;
        }
        redraw
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) -> Redraw {
        self.interface
            .after_frame(&mut state.interface, cx, deadline)
    }

    fn preview_content<'a>(&'a self, state: &'a Self::State) -> Content<'a> {
        if state.displayed.is_some() {
            Content::Ready(&state.pane)
        } else {
            Content::Empty
        }
    }

    fn preview_event(
        &mut self,
        state: &mut Self::State,
        event: PreviewEvent,
        cx: &Context<'_, T>,
    ) -> Redraw {
        let Some(item) = cx.hovered() else {
            return Redraw::NONE;
        };
        if state.displayed != Some(item.id()) {
            return Redraw::NONE;
        }
        if matches!(event, PreviewEvent::Refresh) {
            state.refresh = true;
            return Redraw::NONE;
        }

        let pane = &mut state.pane;
        let previous = (pane.offset(), pane.line_numbers());
        match event {
            PreviewEvent::ToggleLineNumbers => pane.toggle_line_numbers(),
            PreviewEvent::SetLineNumbers(enabled) => pane.set_line_numbers(enabled),
            _ if state.size.width == 0 || state.size.height == 0 => return Redraw::NONE,
            event => {
                let mut viewport = pane.viewport(state.size);
                match event {
                    PreviewEvent::Left(columns) => viewport.left(columns),
                    PreviewEvent::Right(columns) => viewport.right(columns),
                    PreviewEvent::Up(lines) => viewport.up(lines),
                    PreviewEvent::Down(lines) => viewport.down(lines),
                    PreviewEvent::PageUp(pages) => viewport.page_up(pages),
                    PreviewEvent::PageDown(pages) => viewport.page_down(pages),
                    PreviewEvent::AlignLeft => viewport.align_left(),
                    PreviewEvent::AlignRight => viewport.align_right(),
                    PreviewEvent::AlignTop => viewport.align_top(),
                    PreviewEvent::AlignBottom => viewport.align_bottom(),
                    PreviewEvent::Refresh
                    | PreviewEvent::ToggleLineNumbers
                    | PreviewEvent::SetLineNumbers(_) => unreachable!(),
                };
            }
        }
        if (pane.offset(), pane.line_numbers()) != previous {
            Redraw::PREVIEW
        } else {
            Redraw::NONE
        }
    }

    fn preview_resize(&mut self, state: &mut Self::State, size: Option<Dimensions>) {
        state.size = size.unwrap_or_default();
        if state.size.height != 0 {
            state.pane.viewport(state.size).clamp_vertical();
        }
    }
}

impl<I: KeybindDefault, F> KeybindDefault for UncachedPreview<I, F> {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key).or_else(|| keybind_passthrough_preview(key).ok())
    }
}

#[cfg(test)]
mod tests;
