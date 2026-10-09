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

use crate::rect::{Area, Position, Rect};

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

pub(crate) trait PreviewComponent<T> {
    type Error;

    // this is a bit hacky: mostly we try to feature-gate the preview component but
    // if the preview feature is enabled, one can still use the usual picker, in which case
    // we don't want any runtime cost
    const ENABLED: bool;

    /// Handle a preview event
    #[cfg(feature = "preview")]
    fn handle(&mut self, event: PreviewEvent, selected_id: Option<u32>);

    fn update(
        &mut self,
        selected: Option<u32>,
        snapshot: &nucleo::Snapshot<T>,
        deadline: Instant,
        was_enabled: bool,
    ) -> Result<bool, Self::Error>;

    /// Do extra processing when a preview frame is hidden.
    fn hide(&mut self);

    /// Do extra processing when a restart event is received.
    fn restart(&mut self);
}

#[cfg(feature = "preview")]
use crate::preview::PreviewEvent;

pub(crate) struct NoPreview<A>(std::marker::PhantomData<fn() -> A>);

impl<A> NoPreview<A> {
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<E, A> Component<E> for NoPreview<A> {
    fn resize(&mut self, _area: Area, _engine: &E) {}
    fn draw<D: Rect>(&mut self, _engine: &E, _rect: &mut D) -> io::Result<()> {
        Ok(())
    }
}

impl<T, A> PreviewComponent<T> for NoPreview<A> {
    type Error = A;
    const ENABLED: bool = false;
    #[cfg(feature = "preview")]
    fn handle(&mut self, _event: PreviewEvent, _selected_id: Option<u32>) {}
    fn update(
        &mut self,
        _selected: Option<u32>,
        _snapshot: &nucleo::Snapshot<T>,
        _deadline: Instant,
        _was_enabled: bool,
    ) -> Result<bool, A> {
        Ok(false)
    }
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
    type Error = C::Error;
    const ENABLED: bool = C::ENABLED;
    #[cfg(feature = "preview")]
    fn handle(&mut self, event: PreviewEvent, selected_id: Option<u32>) {
        (**self).handle(event, selected_id);
    }
    fn update(
        &mut self,
        selected: Option<u32>,
        snapshot: &nucleo::Snapshot<T>,
        deadline: Instant,
        was_enabled: bool,
    ) -> Result<bool, Self::Error> {
        (**self).update(selected, snapshot, deadline, was_enabled)
    }
    fn hide(&mut self) {
        (**self).hide();
    }
    fn restart(&mut self) {
        (**self).restart();
    }
}
