use std::io::{self, BufWriter, IsTerminal};

use crossterm::event::KeyEvent;

use super::{Preview, pane::PreviewPane};
#[cfg(feature = "unstable-backend")]
use crate::Terminal;
use crate::{
    MultiPicker, Pick, Picker, Render, Selection,
    error::PickError,
    event::{Event, EventSource, StdinReader, keybind_default, keybind_no_multi},
    match_list::SelectedIndices,
    pick::{delegate_picker_methods, sealed},
    terminal::CrosstermTerminal,
};

/// A picker which also generates a preview pane for items.
///
/// Initialize this by first constructing a [`Picker`], then calling [`Picker::with_preview`].
/// There is also a multi-selection variant at [`PreviewMultiPicker`] for multiple selections,
/// obtained either with [`PreviewPicker::multi`] or [`MultiPicker::with_preview`].
///
/// Broadly speaking, this struct is similar to a [`Picker`] albeit with additional support for
/// preview pane rendering. Here we only document the additional preview-specific features.
///
/// # Preview cache
///
/// The picker stores generated previews along with scroll position in an LRU cache. This means that
/// the the internal previewer may not be called to generate the preview pane if previewing already
/// succeeded earlier. The cache is cleared when processing a [restart event](Event::Restart).
/// See the [`Preview`] docs for more detail.
///
/// The buffers are reused within a given session and are dropped on exit or restart.
///
/// When the preview is hidden (for instance, with
/// [`LayoutEvent::TogglePreview`](crate::event::LayoutEvent::TogglePreview)), the cache is preserved
/// and pending requests continue to be handled, but no new preview requests will be made
/// automatically. Note that all preview-related events (such as pane movement and refresh) are
/// ignored while hidden.
pub struct PreviewPicker<T, R, P> {
    picker: Picker<T, R>,
    previewer: P,
}

impl<T, R, P> PreviewPicker<T, R, P> {
    pub(crate) fn new(picker: Picker<T, R>, previewer: P) -> Self {
        Self { picker, previewer }
    }

    /// Convert into a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn multi(self) -> PreviewMultiPicker<T, R, P> {
        PreviewMultiPicker { picker: self }
    }

    /// Mutably borrow this picker as a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_multi(&mut self) -> &mut PreviewMultiPicker<T, R, P> {
        // SAFETY: PreviewMultiPicker transparently wraps PreviewPicker with identical invariants.
        // The returned reference exclusively reborrows self for the same lifetime.
        unsafe { &mut *std::ptr::from_mut(self).cast::<PreviewMultiPicker<T, R, P>>() }
    }

    /// Convert into a picker by dropping the previewer.
    ///
    /// This is convenient for stateless previewers, or previewers passed by exclusive reference. To
    /// recover the internal previewer as well, use [`into_parts`](Self::into_parts).
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> Picker<T, R> {
        self.picker
    }

    /// Decompose a previewing picker into the constituent picker and previewer.
    #[inline]
    pub fn into_parts(self) -> (Picker<T, R>, P) {
        (self.picker, self.previewer)
    }

    /// Returns a reference to the internal picker.
    #[inline]
    pub fn picker(&self) -> &Picker<T, R> {
        &self.picker
    }

    /// Returns an exclusive reference to the internal picker.
    #[inline]
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        &mut self.picker
    }

    /// Returns a reference to the internal previewer.
    #[inline]
    pub fn previewer(&self) -> &P {
        &self.previewer
    }

    /// Returns an exclusive reference to the internal previewer.
    #[inline]
    pub fn previewer_mut(&mut self) -> &mut P {
        &mut self.previewer
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, P> Extend<T> for PreviewPicker<T, R, P> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.picker.extend(iter);
    }
}

impl<T, R, P> sealed::Sealed for PreviewPicker<T, R, P> {}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> Pick for PreviewPicker<T, R, P> {
    type Item = T;
    type Renderer = R;
    type Output<'a>
        = Option<&'a T>
    where
        Self: 'a;

    delegate_picker_methods!(picker);
}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewPicker<T, R, P> {
    /// Open the interactive picker and return the picked item, if any.
    ///
    /// This method is the same as [`Picker::pick`] while also rendering a preview pane.
    /// Read those docs for more detail.
    #[inline]
    pub fn pick(&mut self) -> Result<Option<&T>, PickError> {
        self.pick_with_keybind(keybind_no_multi)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This is the same as [`Picker::pick_with_keybind`], with additional default keybindings
    /// for the previewer. Read those docs for more detail.
    pub fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Option<&T>, PickError>
    where
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Run the picker interactively with a custom event source, writer, and previewer, returning the selected
    /// item, if any.
    ///
    /// This method is the same as [`Picker::pick_with_io`]; read those docs for more detail.
    #[inline]
    pub fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Option<&T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: io::Write,
    {
        self.picker.pick_impl::<_, _, (), _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            PreviewPane::new(
                &self.picker.preview_config,
                &self.picker.chars,
                &mut self.previewer,
            ),
        )
    }

    /// Run the picker interactively with a custom event source and terminal backend.
    ///
    /// This is the same as [`pick_with_io`](Self::pick_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Option<&T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
    {
        self.picker.pick_impl::<_, _, (), _>(
            event_source,
            terminal,
            PreviewPane::new(
                &self.picker.preview_config,
                &self.picker.chars,
                &mut self.previewer,
            ),
        )
    }
}

/// A picker with multiple selections and item previews.
///
/// This struct can be constructed using [`PreviewPicker::multi`] or [`MultiPicker::with_preview`].
#[repr(transparent)]
pub struct PreviewMultiPicker<T, R, P> {
    picker: PreviewPicker<T, R, P>,
}

impl<T, R, P> PreviewMultiPicker<T, R, P> {
    /// Convert back to a single-selection picker, retaining the internal previewer.
    #[must_use]
    #[inline]
    pub fn single(self) -> PreviewPicker<T, R, P> {
        self.picker
    }

    /// Mutably borrow this picker as a single-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> &mut PreviewPicker<T, R, P> {
        &mut self.picker
    }

    /// Returns a reference to the internal picker.
    #[inline]
    pub fn picker(&self) -> &Picker<T, R> {
        &self.picker.picker
    }

    /// Returns an exclusive reference to the internal picker.
    #[inline]
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        &mut self.picker.picker
    }

    /// Convert into a multi-selection picker by dropping the previewer.
    ///
    /// This is convenient for stateless previewers, or previewers passed by exclusive reference. To
    /// recover the internal previewer as well, use [`into_parts`](Self::into_parts).
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> MultiPicker<T, R> {
        MultiPicker {
            picker: self.picker.picker,
        }
    }

    /// Decompose this picker into its constituent multi-selection picker and previewer.
    #[inline]
    pub fn into_parts(self) -> (MultiPicker<T, R>, P) {
        let (picker, previewer) = self.picker.into_parts();
        (MultiPicker { picker }, previewer)
    }

    /// Returns a reference to the internal previewer.
    #[inline]
    pub fn previewer(&self) -> &P {
        self.picker.previewer()
    }

    /// Returns an exclusive reference to the internal previewer.
    #[inline]
    pub fn previewer_mut(&mut self) -> &mut P {
        self.picker.previewer_mut()
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, P> Extend<T> for PreviewMultiPicker<T, R, P> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.picker.extend(iter);
    }
}

impl<T, R, P> sealed::Sealed for PreviewMultiPicker<T, R, P> {}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> Pick for PreviewMultiPicker<T, R, P> {
    type Item = T;
    type Renderer = R;
    type Output<'a>
        = Selection<'a, T>
    where
        Self: 'a;

    delegate_picker_methods!(picker.picker);
}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewMultiPicker<T, R, P> {
    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method is the same as [`MultiPicker::pick`] with multiple selections and item
    /// previews.
    #[inline]
    pub fn pick(&mut self) -> Result<Selection<'_, T>, PickError> {
        self.pick_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This method is the same as [`Picker::pick_with_keybind`] with multiple selections and item
    /// previews.
    /// Read those docs for more detail.
    pub fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Selection<'_, T>, PickError>
    where
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Run the picker interactively with a custom event source, writer, and previewer, with
    /// multiple selections and item previews.
    ///
    /// This method is the same as [`Picker::pick_with_io`] with multiple selections and item
    /// previews.
    /// Read those docs for more detail.
    #[inline]
    pub fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: io::Write,
    {
        self.picker.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            PreviewPane::new(
                &self.picker.picker.preview_config,
                &self.picker.picker.chars,
                &mut self.picker.previewer,
            ),
        )
    }

    /// Run the picker interactively with a custom event source and terminal backend, allowing the
    /// user to select multiple items.
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
    {
        self.picker.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            terminal,
            PreviewPane::new(
                &self.picker.picker.preview_config,
                &self.picker.picker.chars,
                &mut self.picker.previewer,
            ),
        )
    }
}
