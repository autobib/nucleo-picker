use super::{Draw, SessionState, dimensions};
use crate::{
    Render,
    draw::Surface,
    interface::Interface,
    preview::{BoundaryChars, Content, content_dimensions},
    rect::Dimensions,
};

/// A view of the preview pane component of a prepared frame for drawing, resize operations,
/// and introspection.
pub struct PreparedPreview<'frame, 'items, T, R, I: Interface<T>> {
    pub(super) session: &'frame mut SessionState<'items, T, R, I>,
    pub(super) border: Option<BoundaryChars>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedPreview<'_, '_, T, R, I> {
    /// Returns the current contents of the preview pane.
    ///
    /// Internally, this calls [`Interface::preview_content`] unless this component is hidden in
    /// which case it will always return [`Content::Empty`].
    pub fn content(&self) -> Content<'_> {
        if self.session.active_components.preview {
            self.session.interface.preview_content(&self.session.state)
        } else {
            Content::Empty
        }
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> PreparedPreview<'_, '_, T, R, I> {
    /// Set border characters for use in the preview.
    ///
    /// This can be convenient when the provided surface does not have a nice API for drawing the
    /// border. Setting border characters will mean that the picker automatically draws a border in
    /// the rectangle in which it is drawn.
    #[must_use]
    pub fn border(mut self, chars: BoundaryChars) -> Self {
        self.border = Some(chars);
        self
    }

    /// Resize the preview pane.
    ///
    /// This updates the size of the prompt. If the resize is
    /// necessary, this will call the [`Interface::preview_resize`] hook.
    ///
    /// Resizing automatically occurs when [`draw`](Self::draw) is called, so this
    /// method should only be called if implementing custom draw logic.
    pub fn resize(&mut self, size: Dimensions) {
        Draw::resize(self, size);
    }

    /// Draw the preview pane on the provided surface.
    ///
    /// This internally performs a resize to match the target dimensions if necessary. If the resize
    /// is necessary, this will call the [`Interface::preview_resize`] hook. If the
    /// surface is empty or hidden, the resize may still apply even if no drawing operations are
    /// emitted.
    pub fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::draw(self, surface)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Draw
    for PreparedPreview<'_, '_, T, R, I>
{
    fn resize(&mut self, size: Dimensions) {
        let session = &mut *self.session;
        if !I::PREVIEW {
            return;
        }
        let delivered = session
            .active_components
            .preview
            .then(|| content_dimensions(size, self.border.is_some()));
        if session.preview_resize != Some(delivered) {
            session
                .interface
                .preview_resize(&mut session.state, delivered);
            session.preview_resize = Some(delivered);
        }
    }

    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        Draw::resize(self, dimensions(surface));
        if surface.is_empty() || !self.session.active_components.preview {
            return Ok(());
        }
        crate::preview::draw(
            surface,
            self.content(),
            self.border,
            self.session.chars.ellipsis,
        )
    }
}

#[cfg(feature = "ratatui")]
#[cfg_attr(docsrs, doc(cfg(feature = "ratatui")))]
impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> ratatui_core::widgets::Widget
    for PreparedPreview<'_, '_, T, R, I>
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
