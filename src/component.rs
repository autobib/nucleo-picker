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
use crate::draw::Draw;
use crate::rect::{Area, Position};
use crate::{interface::Interface, preview::BoundaryChars};

/// A single component in the frame.
pub(crate) trait Component {
    /// Resize the component, potentially taking into account the match data from the internal engine.
    fn resize(&mut self, area: Area);

    /// Draw this component in the given rectangle taking into account the match data from the
    /// internal engine. Make sure to take into account the rectangle drawing constraints.
    fn draw<D: Draw>(&mut self, rect: &mut D) -> Result<(), D::Error>;

    fn cursor(&self) -> Option<Position> {
        None
    }
}

pub(crate) struct PreviewPane<'a, T, I: Interface<T>> {
    pub interface: &'a I,
    pub state: &'a I::State,
    pub boundary_chars: BoundaryChars,
    pub ellipsis: char,
}

impl<T, I: Interface<T>> Component for PreviewPane<'_, T, I> {
    fn resize(&mut self, _area: Area) {}

    fn draw<D: Draw>(&mut self, rect: &mut D) -> Result<(), D::Error> {
        crate::preview::draw(
            rect,
            self.interface.preview_content(self.state),
            self.boundary_chars,
            self.ellipsis,
        )
    }
}

impl<C: Component> Component for &mut C {
    fn resize(&mut self, area: Area) {
        (**self).resize(area);
    }
    fn draw<D: Draw>(&mut self, rect: &mut D) -> Result<(), D::Error> {
        (**self).draw(rect)
    }
    fn cursor(&self) -> Option<Position> {
        (**self).cursor()
    }
}
