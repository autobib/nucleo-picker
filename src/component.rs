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
    PickerChars,
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

pub(crate) trait PreviewComponent<T> {
    type State;

    // this is a bit hacky: mostly we try to feature-gate the preview component but
    // if the preview feature is enabled, one can still use the usual picker, in which case
    // we don't want any runtime cost
    const ENABLED: bool = false;

    fn init(&mut self) -> Self::State;

    fn component(
        &mut self,
        _state: &mut Self::State,
        _chars: &PickerChars,
        #[cfg(feature = "preview")] _config: &PreviewConfig,
    ) -> impl Component<()> {
        NoPreview
    }

    /// Handle a preview event
    #[cfg(feature = "preview")]
    fn handle(&mut self, _state: &mut Self::State, _event: PreviewEvent, _cx: &Context<'_, T>) {}

    fn update(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _matches_changed: bool,
        _deadline: Instant,
    ) -> bool {
        false
    }

    fn prefetch(&mut self, _state: &mut Self::State, _cx: &Context<'_, T>, _deadline: Instant) {}

    /// Do extra processing when a preview frame is hidden.
    fn hide(&mut self, _state: &mut Self::State) {}

    /// Do extra processing when a restart event is received.
    fn restart(&mut self, _state: &mut Self::State) {}

    fn end(&mut self, _state: &mut Self::State) {}
}

#[cfg(feature = "preview")]
use crate::preview::{PreviewConfig, PreviewEvent};

pub(crate) struct NoPreview;

impl Component<()> for NoPreview {
    fn resize(&mut self, _area: Area, _engine: &()) {}
    fn draw<D: Rect>(&mut self, _engine: &(), _rect: &mut D) -> io::Result<()> {
        Ok(())
    }
}

impl<T> PreviewComponent<T> for NoPreview {
    type State = ();

    fn init(&mut self) {}
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
