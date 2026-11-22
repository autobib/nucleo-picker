use super::{Draw, SessionRead, SessionState};
use crate::{
    Render,
    component::Component,
    draw::Surface,
    interface::Interface,
    rect::Dimensions,
    session::{MatchStatus, QueueStatus},
};

/// The status bar in a prepared frame.
pub struct PreparedStatus<'frame, 'items, T, R, I: Interface<T>> {
    pub(super) session: &'frame mut SessionState<'items, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionRead for PreparedStatus<'_, '_, T, R, I> {
    type Item = T;
    type Renderer = R;
    type Interface = I;

    fn session(&self) -> &SessionState<'_, T, R, I> {
        self.session
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedStatus<'_, '_, T, R, I> {
    /// Returns the current status of matching progress.
    ///
    /// This forwards to [`Session::match_status`](super::Session::match_status); see those docs for more details.
    pub fn match_status(&self) -> MatchStatus {
        SessionRead::match_status(self)
    }

    /// Returns the current status of the item queue.
    ///
    /// This forwards to [`Session::queue_status`](super::Session::match_status); see those docs for more details.
    pub fn queue_status(&self) -> Option<QueueStatus> {
        SessionRead::queue_status(self)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> PreparedStatus<'_, '_, T, R, I> {
    /// Resize the status bar.
    ///
    /// This updates the size of the status bar. Note that resizing
    /// automatically occurs when [`draw`](Self::draw) is called, so this method should only be
    /// called if implementing custom draw logic.
    pub fn resize(&mut self, size: Dimensions) {
        Draw::resize(self, size);
    }

    /// Draw the status bar on the provided surface.
    ///
    /// This internally performs a resize to match the target dimensions if necessary. If the
    /// surface is empty or hidden, the resize may still apply even if no drawing operations are
    /// emitted.
    pub fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::draw(self, surface)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Draw
    for PreparedStatus<'_, '_, T, R, I>
{
    fn resize(&mut self, _size: Dimensions) {}

    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        if surface.is_empty() || !self.session.active_components.status {
            return Ok(());
        }
        surface.move_to(0, 0)?;
        self.session.status_line.draw(surface)
    }
}

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> ratatui_core::widgets::Widget
    for PreparedStatus<'_, '_, T, R, I>
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
