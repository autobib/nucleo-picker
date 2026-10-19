use std::{
    io::{self, BufWriter, IsTerminal},
    time::Duration,
};

use crossterm::event::KeyEvent;

#[cfg(feature = "unstable-backend")]
use crate::Terminal;
use crate::{
    Injector, MatchStatus, Matches, Picker, Render, Selection,
    component::NoPreview,
    error::PickError,
    event::{Event, EventSource, Observer, PickerStatus, StdinReader, keybind_default},
    match_list::SelectedIndices,
    terminal::CrosstermTerminal,
};

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// The shared picker interface.
///
/// This is the interface shared by all picker variants in this crate:
///
/// - [`Picker`]: the default single-selection picker, which also contains these methods as inherent
///   methods (with identical implementation) for convenience.
/// - [`MultiPicker`]: a picker which permits multiple selections
/// - [`PreviewPicker`](crate::preview::PreviewPicker): a picker which renders item previews
/// - [`PreviewMultiPicker`](crate::preview::PreviewMultiPicker): a picker which renders item
///   previews and permits multiple selections.
///
/// This trait is intended for two purposes:
///
/// 1. To make it more convenient to call the [`Picker`] methods on any of the variants instead of
///    having to access the internal single-selection picker.
/// 2. To simplify writing generic code which can accept any picker.
pub trait Pick: sealed::Sealed {
    /// The item type stored in this picker.
    type Item: Send + Sync + 'static;

    /// The renderer used to render the item type for matching and display.
    type Renderer: Render<Self::Item>;

    /// The output of the picker after selection. This is an `Option<&T>` for single-selection
    /// pickers and [`Selection`] for multi-selection pickers.
    type Output<'a>: IntoIterator<Item = &'a Self::Item, IntoIter: ExactSizeIterator>
    where
        Self: 'a;

    /// Run the picker interactively with sensible defaults.
    ///
    /// In all implementations, this method uses default keybindings specialized to the specific picker instance.
    ///
    /// See the documentation for [`Picker::pick`] for more details on the behaviour.
    fn pick(&mut self) -> Result<Self::Output<'_>, PickError>;

    /// Run the picker interactively using custom key-bindings.
    ///
    /// See the documentation for [`Picker::pick_with_keybind`] for more details on the behaviour.
    fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Self::Output<'_>, PickError>
    where
        F: FnMut(KeyEvent) -> Option<Event>;

    /// Run the picker interactively with a custom event source and writer.
    ///
    /// See the documentation for [`Picker::pick_with_io`] for more details on the behaviour.
    fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Self::Output<'_>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: io::Write;

    /// Run the picker interactively with a custom event source and generic terminal backend.
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Self::Output<'_>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal;

    /// Get an [`Injector`] to send items to the picker.
    ///
    /// This is the same as [`Picker::injector`].
    fn injector(&self) -> Injector<Self::Item, Self::Renderer>;

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// This is the same as [`Picker::push_batch`].
    fn push_batch<I>(&self, items: I)
    where
        I: IntoIterator<Item = Self::Item>;

    /// Returns the current value of the query in the picker prompt.
    ///
    /// This is the same as [`Picker::query`].
    fn query(&self) -> &str;

    /// Update the default query string.
    ///
    /// This is the same as [`Picker::update_query`].
    fn update_query<Q>(&mut self, query: Q)
    where
        Q: Into<String>;

    /// Reset the query string to the empty string.
    ///
    /// This is the same as [`Picker::reset_query`].
    fn reset_query(&mut self);

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// This is the same as [`Picker::restart`].
    fn restart(&mut self);

    /// Restart the match engine, disconnecting all active injectors and replacing the internal
    /// renderer.
    ///
    /// This is the same as [`Picker::reset_renderer`].
    fn reset_renderer(&mut self, renderer: Self::Renderer);

    /// Returns an [`Observer`] containing up-to-date [`Injector`]s for this picker.
    ///
    /// This is the same channel as returned by [`Picker::injector_observer`]. See those docs for
    /// more detail.
    #[must_use]
    fn injector_observer(
        &mut self,
        with_injector: bool,
    ) -> Observer<Injector<Self::Item, Self::Renderer>>;

    /// Returns an [`Observer`] containing up-to-date responses from [status
    /// requests](crate::event::Event::Status).
    ///
    /// This is the same as [`Picker::status_observer`].
    #[must_use]
    fn status_observer(&mut self) -> Observer<PickerStatus>;

    /// Filter the items sent to the picker using the current value of the query.
    ///
    /// This requires that there are no live injectors. See [`Picker::filter`] for more details.
    fn filter(&mut self) -> Option<Matches<'_, Self::Item>>;

    /// Advance the internal match engine state non-interactively.
    ///
    /// This is the same as [`Picker::update_matches`].
    fn update_matches(&mut self, wait: Duration) -> MatchStatus;

    /// Returns a view of the current matches.
    ///
    /// This is the same as [`Picker::matches`].
    #[must_use]
    fn matches(&self) -> Matches<'_, Self::Item>;

    /// A convenience method to obtain the rendered version of an item as it would appear in the
    /// picker.
    fn render<'a>(&self, item: &'a Self::Item) -> <Self::Renderer as Render<Self::Item>>::Str<'a>;
}

macro_rules! delegate_picker_methods {
    ($($field:ident).*) => {
        #[inline]
        fn injector(&self) -> crate::Injector<T, R> {
            crate::Picker::injector(&self$(.$field)*)
        }

        #[inline]
        fn push_batch<I: IntoIterator<Item = T>>(&self, items: I) {
            crate::Picker::push_batch(&self$(.$field)*, items);
        }

        #[inline]
        fn query(&self) -> &str {
            crate::Picker::query(&self$(.$field)*)
        }

        #[inline]
        fn update_query<Q: Into<String>>(&mut self, query: Q) {
            crate::Picker::update_query(&mut (*self)$(.$field)*, query);
        }

        #[inline]
        fn reset_query(&mut self) {
            crate::Picker::reset_query(&mut (*self)$(.$field)*);
        }

        #[inline]
        fn restart(&mut self) {
            crate::Picker::restart(&mut (*self)$(.$field)*);
        }

        #[inline]
        fn reset_renderer(&mut self, renderer: R) {
            crate::Picker::reset_renderer(&mut (*self)$(.$field)*, renderer);
        }

        #[inline]
        fn injector_observer(&mut self, with_injector: bool) -> crate::event::Observer<crate::Injector<T, R>> {
            crate::Picker::injector_observer(&mut (*self)$(.$field)*, with_injector)
        }

        #[inline]
        fn status_observer(&mut self) -> crate::event::Observer<crate::event::PickerStatus> {
            crate::Picker::status_observer(&mut (*self)$(.$field)*)
        }

        #[inline]
        fn filter(&mut self) -> Option<crate::Matches<'_, T>> {
            crate::Picker::filter(&mut (*self)$(.$field)*)
        }

        #[inline]
        fn update_matches(&mut self, wait: std::time::Duration) -> crate::MatchStatus {
            crate::Picker::update_matches(&mut (*self)$(.$field)*, wait)
        }

        #[inline]
        fn matches(&self) -> crate::Matches<'_, T> {
            crate::Picker::matches(&self$(.$field)*)
        }

        #[inline]
        fn render<'a>(&self, item: &'a T) -> <R as crate::Render<T>>::Str<'a> {
            crate::Picker::render(&self$(.$field)*, item)
        }

        #[inline]
        fn pick(&mut self) -> Result<Self::Output<'_>, crate::error::PickError> {
            Self::pick(self)
        }

        #[inline]
        fn pick_with_keybind<F>(
            &mut self,
            keybind: F,
        ) -> Result<Self::Output<'_>, crate::error::PickError>
        where
            F: FnMut(crossterm::event::KeyEvent) -> Option<crate::event::Event>,
        {
            Self::pick_with_keybind(self, keybind)
        }

        #[inline]
        fn pick_with_io<E: crate::event::EventSource, W: std::io::Write>(
            &mut self,
            event_source: E,
            writer: &mut W,
        ) -> Result<Self::Output<'_>, crate::error::PickError<E::AbortErr>> {
            Self::pick_with_io(self, event_source, writer)
        }

        #[cfg(feature = "unstable-backend")]
        #[doc(hidden)]
        #[inline]
        fn pick_with_terminal_io<E: crate::event::EventSource, W: crate::Terminal>(
            &mut self,
            event_source: E,
            terminal: &mut W,
        ) -> Result<Self::Output<'_>, crate::error::PickError<E::AbortErr>> {
            Self::pick_with_terminal_io(self, event_source, terminal)
        }
    };
}

#[cfg(feature = "preview")]
pub(crate) use delegate_picker_methods;

impl<T, R> sealed::Sealed for Picker<T, R> {}

impl<T: Send + Sync + 'static, R: Render<T>> Pick for Picker<T, R> {
    type Item = T;
    type Renderer = R;
    type Output<'a>
        = Option<&'a T>
    where
        Self: 'a;

    delegate_picker_methods!();
}

/// A picker with multiple selections enabled.
///
/// This is the same as a [`Picker`], but enables multiple selections. Obtain this struct by
/// calling the [`Picker::multi`] method. A mutably borrowed version can also be obtained with
/// [`Picker::as_multi`].
#[repr(transparent)]
pub struct MultiPicker<T, R> {
    pub(crate) picker: Picker<T, R>,
}

impl<T, R> MultiPicker<T, R> {
    /// Convert back to a single-selection picker.
    #[must_use]
    #[inline]
    pub fn single(self) -> Picker<T, R> {
        self.picker
    }

    /// Mutably borrow this picker as a single-selection picker.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> &mut Picker<T, R> {
        &mut self.picker
    }

    /// Returns a reference to the internal single-selection picker.
    #[must_use]
    #[inline]
    pub fn picker(&self) -> &Picker<T, R> {
        &self.picker
    }

    /// Returns an exclusive reference to the internal single-selection picker.
    #[must_use]
    #[inline]
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        &mut self.picker
    }

    /// Get a multi-picker variant which also generates previews.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[must_use]
    #[inline]
    pub fn with_preview<P: crate::preview::Preview<T>>(
        self,
        previewer: P,
    ) -> crate::PreviewMultiPicker<T, R, P> {
        crate::PreviewPicker::new(self.picker, previewer).multi()
    }
}

impl<T: Send + Sync + 'static, R: Render<T>> Extend<T> for MultiPicker<T, R> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.push_batch(iter);
    }
}

impl<T, R> sealed::Sealed for MultiPicker<T, R> {}

impl<T: Send + Sync + 'static, R: Render<T>> Pick for MultiPicker<T, R> {
    type Item = T;
    type Renderer = R;
    type Output<'a>
        = Selection<'a, T>
    where
        Self: 'a;

    delegate_picker_methods!(picker);
}

impl<T: Send + Sync + 'static, R: Render<T>> MultiPicker<T, R> {
    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`Picker::pick`]. See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    pub fn pick(&mut self) -> Result<Selection<'_, T>, PickError> {
        self.pick_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`Picker::pick_with_keybind`]. See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
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

    /// Run the picker interactively with a custom event source and writer, allowing the user to
    /// select multiple items.
    ///
    /// This is otherwise identical to [`Picker::pick_with_io`]; see those docs as well
    /// as the [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    pub fn pick_with_io<E: EventSource, W: io::Write>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>> {
        self.picker.pick_impl::<_, _, SelectedIndices, _>(
            event_source,
            &mut CrosstermTerminal::new(writer),
            NoPreview,
        )
    }

    /// Run the picker interactively with a custom event source and terminal backend, allowing the
    /// user to select multiple items.
    ///
    /// This is the same as [`Picker::pick_with_terminal_io`] but in addition allows multiple
    /// selections.
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<E: EventSource, W: Terminal>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Selection<'_, T>, PickError<E::AbortErr>> {
        self.picker
            .pick_impl::<_, _, SelectedIndices, _>(event_source, terminal, NoPreview)
    }
}

impl<P: Pick + ?Sized> sealed::Sealed for &mut P {}

impl<P: Pick + ?Sized> Pick for &mut P {
    type Item = P::Item;
    type Renderer = P::Renderer;
    type Output<'a>
        = P::Output<'a>
    where
        Self: 'a;

    #[inline]
    fn pick(&mut self) -> Result<Self::Output<'_>, PickError> {
        P::pick(&mut **self)
    }

    #[inline]
    fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Self::Output<'_>, PickError>
    where
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        P::pick_with_keybind(&mut **self, keybind)
    }

    #[inline]
    fn pick_with_io<E: EventSource, W: io::Write>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Self::Output<'_>, PickError<E::AbortErr>> {
        P::pick_with_io(&mut **self, event_source, writer)
    }

    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    fn pick_with_terminal_io<E: EventSource, W: Terminal>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Self::Output<'_>, PickError<E::AbortErr>> {
        P::pick_with_terminal_io(&mut **self, event_source, terminal)
    }

    #[inline]
    fn injector(&self) -> Injector<Self::Item, Self::Renderer> {
        P::injector(&**self)
    }

    #[inline]
    fn push_batch<I: IntoIterator<Item = Self::Item>>(&self, items: I) {
        P::push_batch(&**self, items);
    }

    #[inline]
    fn query(&self) -> &str {
        P::query(&**self)
    }

    #[inline]
    fn update_query<Q: Into<String>>(&mut self, query: Q) {
        P::update_query(&mut **self, query);
    }

    #[inline]
    fn reset_query(&mut self) {
        P::reset_query(&mut **self);
    }

    #[inline]
    fn restart(&mut self) {
        P::restart(&mut **self);
    }

    #[inline]
    fn reset_renderer(&mut self, renderer: Self::Renderer) {
        P::reset_renderer(&mut **self, renderer);
    }

    #[inline]
    fn injector_observer(
        &mut self,
        with_injector: bool,
    ) -> Observer<Injector<Self::Item, Self::Renderer>> {
        P::injector_observer(&mut **self, with_injector)
    }

    #[inline]
    fn status_observer(&mut self) -> Observer<PickerStatus> {
        P::status_observer(&mut **self)
    }

    #[inline]
    fn filter(&mut self) -> Option<Matches<'_, Self::Item>> {
        P::filter(&mut **self)
    }

    #[inline]
    fn update_matches(&mut self, wait: Duration) -> MatchStatus {
        P::update_matches(&mut **self, wait)
    }

    #[inline]
    fn matches(&self) -> Matches<'_, Self::Item> {
        P::matches(&**self)
    }

    #[inline]
    fn render<'a>(&self, item: &'a Self::Item) -> <Self::Renderer as Render<Self::Item>>::Str<'a> {
        P::render(&**self, item)
    }
}
