//! # The component abstraction
//!
//! Each component is only initialized for a single picker session. The component is given read-only
//! access to the underlying match engine and is responsible for component-specific event handling.
//!
//! A [`Component`] is a single part of the terminal interface. There are three core components:
//!
//! - the match list
//! - the status line
//! - the prompt
//!
//! as well as an additional component when enabled:
//!
//! - the previewer
//!
use std::{io, time::Instant};

use crate::{
    rect::{Area, Position, Rect},
    session::Context,
};

/// A single component in the frame.
pub(crate) trait Component<E> {
    /// Resize the component, potentially taking into account the match data from the internal engine.
    fn resize(&mut self, area: Area, engine: &E);

    /// Draw this component in the given rectangle taking into account the match data from the
    /// internal engine. Make sure to take into account the rectangle drawing constraints.
    fn draw<D: Rect>(&mut self, engine: &E, rect: &mut D) -> io::Result<()>;

    fn cursor(&self, _engine: &E) -> Option<Position> {
        None
    }
}

pub(crate) trait PreviewComponent<T>: Component<()> {
    // this is a bit hacky: mostly we try to feature-gate the preview component but
    // if the preview feature is enabled, one can still use the usual picker, in which case
    // we don't want any runtime cost
    const ENABLED: bool;

    /// Handle a preview event
    #[cfg(feature = "preview")]
    fn handle(&mut self, event: PreviewEvent, cx: &Context<'_, T>);

    fn update(&mut self, cx: &Context<'_, T>, deadline: Instant) -> bool;

    fn prefetch(&mut self, cx: &Context<'_, T>, deadline: Instant);

    /// Do extra processing when a preview frame is hidden.
    fn hide(&mut self);

    /// Do extra processing when a restart event is received.
    fn restart(&mut self);
}

#[cfg(feature = "preview")]
use crate::preview::PreviewEvent;

pub(crate) struct NoPreview;

impl Component<()> for NoPreview {
    fn resize(&mut self, _area: Area, _engine: &()) {}
    fn draw<D: Rect>(&mut self, _engine: &(), _rect: &mut D) -> io::Result<()> {
        Ok(())
    }
}

impl<T> PreviewComponent<T> for NoPreview {
    const ENABLED: bool = false;
    #[cfg(feature = "preview")]
    fn handle(&mut self, _event: PreviewEvent, _cx: &Context<'_, T>) {}
    fn update(&mut self, _cx: &Context<'_, T>, _deadline: Instant) -> bool {
        false
    }
    fn prefetch(&mut self, _cx: &Context<'_, T>, _deadline: Instant) {}
    fn hide(&mut self) {}
    fn restart(&mut self) {}
}

#[cfg(test)]
impl<E, C: Component<E>> Component<E> for &mut C {
    fn resize(&mut self, area: Area, engine: &E) {
        (**self).resize(area, engine);
    }
    fn draw<D: Rect>(&mut self, engine: &E, rect: &mut D) -> io::Result<()> {
        (**self).draw(engine, rect)
    }
    fn cursor(&self, engine: &E) -> Option<Position> {
        (**self).cursor(engine)
    }
}
#[cfg(test)]
impl<T, C: PreviewComponent<T>> PreviewComponent<T> for &mut C {
    const ENABLED: bool = C::ENABLED;
    #[cfg(feature = "preview")]
    fn handle(&mut self, event: PreviewEvent, cx: &Context<'_, T>) {
        (**self).handle(event, cx);
    }
    fn update(&mut self, cx: &Context<'_, T>, deadline: Instant) -> bool {
        (**self).update(cx, deadline)
    }
    fn prefetch(&mut self, cx: &Context<'_, T>, deadline: Instant) {
        (**self).prefetch(cx, deadline);
    }
    fn hide(&mut self) {
        (**self).hide();
    }
    fn restart(&mut self) {
        (**self).restart();
    }
}
