use super::{Draw, SessionRead, SessionState, dimensions};
use crate::{
    Render,
    component::Component,
    draw::Surface,
    interface::Interface,
    rect::Dimensions,
    session::{ItemRef, Matches},
};

/// A view of the match list component of a prepared frame for drawing, resize operations,
/// and introspection.
pub struct PreparedMatchList<'frame, 'items, T, R, I: Interface<T>> {
    pub(super) session: &'frame mut SessionState<'items, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionRead
    for PreparedMatchList<'_, '_, T, R, I>
{
    type Item = T;
    type Renderer = R;
    type Interface = I;

    fn session(&self) -> &SessionState<'_, T, R, I> {
        self.session
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedMatchList<'_, '_, T, R, I> {
    /// Returns a read-only view over the items in the match list.
    pub fn matches(&self) -> Matches<'_, T> {
        SessionRead::matches(self)
    }

    /// Returns the rank corresponding to the current hovered item in the match list, or `None` if
    /// there are no matches.
    pub fn hovered_rank(&self) -> Option<u32> {
        SessionRead::hovered_rank(self)
    }

    /// Returns the item at the committed hovered rank, or `None` if there are no matches.
    pub fn hovered(&self) -> Option<ItemRef<'_, T>> {
        SessionRead::hovered(self)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> PreparedMatchList<'_, '_, T, R, I> {
    /// Resize the match list.
    ///
    /// This updates the size of the match list. Note that resizing
    /// automatically occurs when [`draw`](Self::draw) is called, so this method should only be
    /// called if implementing custom draw logic.
    pub fn resize(&mut self, size: Dimensions) {
        Draw::resize(self, size);
    }

    /// Draw the match list on the provided surface.
    ///
    /// This internally performs a resize to match the target dimensions if necessary. If the
    /// surface is empty or hidden, the resize may still apply even if no drawing operations are
    /// emitted.
    pub fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::draw(self, surface)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Draw
    for PreparedMatchList<'_, '_, T, R, I>
{
    fn resize(&mut self, size: Dimensions) {
        self.session.list().resize(size);
    }

    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::resize(self, dimensions(surface));
        if surface.is_empty() || !self.session.active_components.match_list {
            return Ok(());
        }
        self.session.list().draw(surface)
    }
}

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> ratatui_core::widgets::Widget
    for PreparedMatchList<'_, '_, T, R, I>
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
