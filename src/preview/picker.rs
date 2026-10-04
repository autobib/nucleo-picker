use std::io::{self, BufWriter, IsTerminal};

use crossterm::event::KeyEvent;

use super::{Preview, pane::PreviewPane};
use crate::{
    Picker, Render, Selection, Terminal,
    error::PickError,
    event::{Event, EventSource, StdinReader, keybind_default, keybind_no_multi},
    match_list::SelectedIndices,
    terminal::CrosstermTerminal,
};

/// A picker which also generates a preview pane for items.
///
/// Initialize this by first constructing a [`Picker`], then calling [`Picker::with_preview`].
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
pub struct PreviewPicker<'a, T, R, P> {
    picker: &'a mut Picker<T, R>,
    previewer: P,
}

impl<'a, T, R, P> PreviewPicker<'a, T, R, P> {
    pub(crate) fn new(picker: &'a mut Picker<T, R>, previewer: P) -> Self {
        Self { picker, previewer }
    }

    /// Returns a reference to the internal picker.
    pub fn picker(&self) -> &Picker<T, R> {
        self.picker
    }

    /// Returns an exclusive reference to the internal picker.
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        self.picker
    }

    /// Returns a reference to the internal previewer.
    pub fn previewer(&self) -> &P {
        &self.previewer
    }

    /// Returns an exclusive reference to the internal previewer.
    pub fn previewer_mut(&mut self) -> &mut P {
        &mut self.previewer
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewPicker<'_, T, R, P> {
    /// Open the interactive picker and return the picked item, if any.
    ///
    /// This method is the same as [`Picker::pick`] while also rendering a preview pane.
    /// Read those docs for more detail.
    ///
    /// # Errors
    ///
    /// In addition to the errors which are described in [`Picker::pick`], an error from
    /// [`Preview::preview`] will immediately terminate the picker with [`PickError::Aborted`].
    /// Read the [`Preview`] docs for more detail about preview pane errors.
    pub fn pick(&mut self) -> Result<Option<&T>, PickError<P::AbortErr>> {
        self.pick_with_keybind(keybind_no_multi)
    }

    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method is the same as [`Picker::pick_multi`] while also rendering a preview pane.
    /// Read those docs for more detail.
    pub fn pick_multi(&mut self) -> Result<Selection<'_, T>, PickError<P::AbortErr>> {
        self.pick_multi_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This is the same as [`Picker::pick_with_keybind`], with additional default keybindings
    /// for the previewer. Read those docs for more detail.
    pub fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Option<&T>, PickError<P::AbortErr>>
    where
        F: FnMut(KeyEvent) -> Option<Event<P::AbortErr>>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`pick_with_keybind`](Self::pick_with_keybind). See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    pub fn pick_multi_with_keybind<F>(
        &mut self,
        keybind: F,
    ) -> Result<Selection<'_, T>, PickError<P::AbortErr>>
    where
        F: FnMut(KeyEvent) -> Option<Event<P::AbortErr>>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_multi_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Run the picker interactively with a custom event source, writer, and previewer, returning the selected
    /// item, if any.
    ///
    /// This method is the same as [`Picker::pick_with_io`]; read those docs for more detail.
    ///
    /// # Errors
    ///
    /// The event source and previewer must use the same abort error type. In addition to the
    /// errors documented in [`Picker::pick_with_io`], previewer errors are also propagated as
    /// [`PickError::Aborted`].
    pub fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Option<&T>, PickError<P::AbortErr>>
    where
        E: EventSource<AbortErr = P::AbortErr>,
        W: io::Write,
    {
        self.picker.pick_impl::<_, _, (), _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            PreviewPane::new(&self.picker.preview_config, &mut self.previewer),
        )
    }

    /// Run the picker interactively with a custom event source, writer, and previewer, allowing the user to
    /// select multiple items.
    ///
    /// This is otherwise identical to [`pick_with_io`](Self::pick_with_io); see those docs as well
    /// as the [docs on multiple selections](Picker#multiple-selections) for more detail.
    pub fn pick_multi_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Selection<'_, T>, PickError<P::AbortErr>>
    where
        E: EventSource<AbortErr = P::AbortErr>,
        W: io::Write,
    {
        self.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            PreviewPane::new(&self.picker.preview_config, &mut self.previewer),
        )
    }
}

#[doc(hidden)]
impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewPicker<'_, T, R, P> {
    /// Run the picker interactively with a custom event source and terminal backend.
    ///
    /// This is the same as [`pick_with_io`](Self::pick_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[cfg(feature = "unstable-backend")]
    pub fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Option<&T>, PickError<P::AbortErr>>
    where
        E: EventSource<AbortErr = P::AbortErr>,
        W: Terminal,
    {
        self.picker.pick_impl::<_, _, (), _>(
            event_source,
            terminal,
            PreviewPane::new(&self.picker.preview_config, &mut self.previewer),
        )
    }

    /// Run the picker interactively with a custom event source and terminal backend, allowing the
    /// user to select multiple items.
    ///
    /// This is the same as [`pick_multi_with_io`](Self::pick_multi_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[cfg(feature = "unstable-backend")]
    pub fn pick_multi_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Selection<'_, T>, PickError<P::AbortErr>>
    where
        E: EventSource<AbortErr = P::AbortErr>,
        W: Terminal,
    {
        self.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            terminal,
            PreviewPane::new(&self.picker.preview_config, &mut self.previewer),
        )
    }
}
