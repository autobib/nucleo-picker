use super::{Draw, SessionRead, SessionState, dimensions};
use crate::{
    Render,
    component::Component,
    draw::Surface,
    interface::Interface,
    rect::{Dimensions, Position},
};

/// The prompt in a prepared frame.
///
/// Like the other prepared types, this struct exposes methods for drawing and resizing, as well as
/// introspection for users which wish to fully customize rendering.
pub struct PreparedPrompt<'frame, 'items, T, R, I: Interface<T>> {
    pub(super) session: &'frame mut SessionState<'items, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionRead for PreparedPrompt<'_, '_, T, R, I> {
    type Item = T;
    type Renderer = R;
    type Interface = I;

    fn session(&self) -> &SessionState<'_, T, R, I> {
        self.session
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedPrompt<'_, '_, T, R, I> {
    /// Returns the query string as visible in the prompt.
    pub fn query(&self) -> &str {
        SessionRead::query(self)
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedPrompt<'_, '_, T, R, I> {
    /// Returns the cursor position in coordinates local to the prompt, or `None` if the prompt is hidden.
    ///
    /// Currently the prompt is always height 1, but this is not guaranteed to always be the
    /// situation in the future.
    pub fn cursor(&self) -> Option<Position> {
        self.session
            .active_components
            .prompt
            .then(|| self.session.prompt.cursor())
            .flatten()
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> PreparedPrompt<'_, '_, T, R, I> {
    /// Resize the prompt.
    ///
    /// This updates the size of the prompt. Note that resizing
    /// automatically occurs when [`draw`](Self::draw) is called, so this method should only be
    /// called if implementing custom draw logic.
    pub fn resize(&mut self, size: Dimensions) {
        Draw::resize(self, size);
    }

    /// Draw the prompt on the provided surface.
    ///
    /// This internally performs a resize to match the target dimensions if necessary. If the
    /// surface is empty or hidden, the resize may still apply even if no drawing operations are
    /// emitted.
    pub fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::draw(self, surface)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Draw
    for PreparedPrompt<'_, '_, T, R, I>
{
    fn resize(&mut self, size: Dimensions) {
        self.session.prompt.resize(size);
    }

    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::resize(self, dimensions(surface));
        if surface.is_empty() || !self.session.active_components.prompt {
            return Ok(());
        }
        surface.move_to(0, 0)?;
        self.session.prompt.draw(surface)
    }
}

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> ratatui_core::widgets::Widget
    for PreparedPrompt<'_, '_, T, R, I>
{
    fn render(
        mut self,
        area: ratatui_core::layout::Rect,
        buffer: &mut ratatui_core::buffer::Buffer,
    ) {
        let Ok(()) = Draw::draw(
            &mut self,
            &mut crate::draw::RatatuiSurface::new(buffer, area),
        );
    }
}
