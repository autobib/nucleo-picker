use std::io::{self, BufWriter, IsTerminal};

use crossterm::event::KeyEvent;

use super::{Preview, pane::PreviewPane};
#[cfg(feature = "unstable-backend")]
use crate::Terminal;
use crate::{
    Injector, Picker, Render, Selection,
    error::PickError,
    event::{
        Event, EventSource, Observer, PickerStatus, StdinReader, keybind_default, keybind_no_multi,
    },
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

    /// Decompose a previewing picker into the constituent picker and previewer.
    pub fn into_parts(self) -> (Picker<T, R>, P) {
        (self.picker, self.previewer)
    }

    /// Returns a reference to the internal picker.
    pub fn picker(&self) -> &Picker<T, R> {
        &self.picker
    }

    /// Returns an exclusive reference to the internal picker.
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        &mut self.picker
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

impl<T: Send + Sync + 'static, R: Render<T>, P> Extend<T> for PreviewPicker<T, R, P> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.picker.extend(iter);
    }
}

impl<T: Send + Sync + 'static, R, P> PreviewPicker<T, R, P> {
    /// Update the default query string.
    ///
    /// See [`Picker::update_query`] for more details.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.picker.update_query(query);
    }

    /// Reset the query string to the empty string.
    ///
    /// See [`Picker::reset_query`] for more details.
    #[inline]
    pub fn reset_query(&mut self) {
        self.picker.reset_query();
    }

    /// Returns the contents of the query string internal to the picker.
    ///
    /// See [`Picker::query`] for more details.
    #[must_use]
    pub fn query(&self) -> &str {
        self.picker.query()
    }

    /// Returns an [`Observer`] containing up-to-date [`Injector`]s for this picker.
    ///
    /// See [`Picker::injector_observer`] for more details.
    #[must_use]
    pub fn injector_observer(&mut self, with_injector: bool) -> Observer<Injector<T, R>> {
        self.picker.injector_observer(with_injector)
    }

    /// Returns an [`Observer`] containing up-to-date responses from [status
    /// requests](Event::Status).
    ///
    /// See [`Picker::status_observer`] for more details.
    #[must_use]
    pub fn status_observer(&mut self) -> Observer<PickerStatus> {
        self.picker.status_observer()
    }

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// See [`Picker::restart`] for more details.
    pub fn restart(&mut self) {
        self.picker.restart();
    }

    /// Restart the match engine, disconnecting all active injectors and replacing the internal
    /// renderer.
    ///
    /// See [`Picker::reset_renderer`] for more details.
    pub fn reset_renderer(&mut self, render: R) {
        self.picker.reset_renderer(render);
    }

    /// Get an [`Injector`] to send items to the picker.
    ///
    /// See [`Picker::injector`] for more details.
    #[must_use]
    pub fn injector(&self) -> Injector<T, R> {
        self.picker.injector()
    }

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// See [`Picker::push_batch`] for more details.
    pub fn push_batch<I>(&self, iter: I)
    where
        R: Render<T>,
        I: IntoIterator<Item = T>,
    {
        self.picker.push_batch(iter);
    }

    /// A convenience method to obtain the rendered version of an item as it would appear in the
    /// picker.
    ///
    /// See [`Picker::render`] for more details.
    #[inline]
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a>
    where
        R: Render<T>,
    {
        self.picker.render(item)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewPicker<T, R, P> {
    /// Open the interactive picker and return the picked item, if any.
    ///
    /// This method is the same as [`Picker::pick`] while also rendering a preview pane.
    /// Read those docs for more detail.
    pub fn pick(&mut self) -> Result<Option<&T>, PickError> {
        self.pick_with_keybind(keybind_no_multi)
    }

    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method is the same as [`Picker::pick_multi`] while also rendering a preview pane.
    /// Read those docs for more detail.
    pub fn pick_multi(&mut self) -> Result<Selection<'_, T>, PickError> {
        self.pick_multi_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This is the same as [`Picker::pick_with_keybind`], with additional default keybindings
    /// for the previewer. Read those docs for more detail.
    pub fn pick_with_keybind<F, A>(&mut self, keybind: F) -> Result<Option<&T>, PickError<A>>
    where
        F: FnMut(KeyEvent) -> Option<Event<A>>,
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
    pub fn pick_multi_with_keybind<F, A>(
        &mut self,
        keybind: F,
    ) -> Result<Selection<'_, T>, PickError<A>>
    where
        F: FnMut(KeyEvent) -> Option<Event<A>>,
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

    /// Run the picker interactively with a custom event source, writer, and previewer, allowing the user to
    /// select multiple items.
    ///
    /// This is otherwise identical to [`pick_with_io`](Self::pick_with_io); see those docs as well
    /// as the [docs on multiple selections](Picker#multiple-selections) for more detail.
    pub fn pick_multi_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: io::Write,
    {
        self.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            PreviewPane::new(
                &self.picker.preview_config,
                &self.picker.chars,
                &mut self.previewer,
            ),
        )
    }
}

#[doc(hidden)]
impl<T: Send + Sync + 'static, R: Render<T>, P: Preview<T>> PreviewPicker<T, R, P> {
    /// Run the picker interactively with a custom event source and terminal backend.
    ///
    /// This is the same as [`pick_with_io`](Self::pick_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[cfg(feature = "unstable-backend")]
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
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
    {
        self.picker.pick_impl::<_, _, SelectedIndices, _>(
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
