use std::io::{self, BufWriter, IsTerminal};

use crossterm::event::KeyEvent;

use crate::{
    PickerCore, Render, Selection, Terminal,
    component::NoPreview,
    error::PickError,
    event::{Event, EventSource, StdinReader, keybind_default, keybind_no_multi},
    match_list::SelectedIndices,
    terminal::CrosstermTerminal,
};

pub(crate) mod sealed {
    use super::*;

    pub trait State<T>: Sized {
        fn default_keybind(key: KeyEvent) -> Option<Event>;

        fn run<'a, R, E, W>(
            &mut self,
            core: &'a mut PickerCore<T, R>,
            event_source: E,
            terminal: &mut W,
        ) -> Result<<Self as Interface<T>>::Output<'a>, PickError<E::AbortErr>>
        where
            T: Send + Sync + 'static,
            Self: Interface<T>,
            R: Render<T>,
            E: EventSource,
            W: Terminal;
    }
}

/// Generics for the picker interface.
///
/// This trait constrains the type parameter `I` of [`Picker`](crate::Picker) when used interactively.
/// Morally, the [`Interface`] defines *the behaviour of the interactive picker*. This includes:
///
/// 1. [`Single`]-selection and [`Multi`]-selection modes, which allows queuing selections in the
///    interactive interface and sets the return type (the `Output` type parameter) as
///    [`Option<&T>`] or [`Selection<'_, T>`] respectively.
/// 2. Preview-enabled pickers (via [`WithPreview`](crate::preview::WithPreview)) which render item
///    previews in a separate pane.
///
/// This trait is sealed and cannot be implemented outside this library.
pub trait Interface<T>: sealed::State<T> {
    /// The output of the picker after selection. This is an [`Option<&T>`] for single-selection
    /// pickers and [`Selection<'_, T>`] for multi-selection pickers.
    type Output<'a>: IntoIterator<Item = &'a T, IntoIter: ExactSizeIterator>
    where
        T: 'a;
}

/// The single-selection picker interface.
///
/// Read the [interface docs](Interface) or the [picker variants docs](crate::Picker#picker-variants) for
/// more details.
pub struct Single;

/// The multiple-selection picker interface.
///
/// Read the [interface docs](Interface) or the [picker variants docs](crate::Picker#picker-variants) for
/// more details.
pub struct Multi;

/// A temporary view of a picker with a different interface.
///
/// The view type is a one-off type with the same [`pick`](Self::pick),
/// [`pick_with_keybind`](Self::pick_with_keybind), and [`pick_with_io`](Self::pick_with_io) methods
/// as a [`Picker`](crate::Picker), with the only difference that these methods take `self` instead of
/// `&mut self` and can therefore only be used a single time.
///
/// This is essentially a wrapper around an `&'a mut Picker`, and the returned output is tied to the
/// `'a` lifetime of the original picker instead of the view type itself.
///
/// ```no_run
/// use nucleo_picker::{Picker, Render, Selection, error::PickError};
///
/// // accept a single-selection picker but run it in multiple selection
/// // mode. note that the returned `Selection` has lifetime tied to the original
/// // picker
/// fn pick_as_multi<'a, T, R>(
///     picker: &'a mut Picker<T, R>,
/// ) -> Result<Selection<'a, T>, PickError>
/// where
///     T: Send + Sync + 'static,
///     R: Render<T>,
/// {
///     picker.as_multi().pick()
/// }
/// ```
pub struct PickerRef<'a, T, R, I = Single> {
    pub(crate) core: &'a mut PickerCore<T, R>,
    pub(crate) interface: I,
}

impl<'a, T, R, I> PickerRef<'a, T, R, I> {
    /// Change the interface of this view type.
    #[must_use]
    #[inline]
    pub fn with_interface<J>(self, interface: J) -> PickerRef<'a, T, R, J> {
        PickerRef {
            core: self.core,
            interface,
        }
    }
}

impl<'a, T, R, I> PickerRef<'a, T, R, I>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
{
    /// Run the picker interactively with sensible defaults.
    ///
    /// See the documentation for [`Picker::pick`](crate::Picker::pick) for more details on the behaviour.
    #[inline]
    pub fn pick(self) -> Result<I::Output<'a>, PickError> {
        self.pick_with_keybind(I::default_keybind)
    }

    /// Run the picker interactively using custom keybindings.
    ///
    /// See the documentation for [`Picker::pick_with_keybind`](crate::Picker::pick_with_keybind) for
    /// more details on the behaviour.
    #[inline]
    pub fn pick_with_keybind<F>(mut self, keybind: F) -> Result<I::Output<'a>, PickError>
    where
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        pick_with_keybind(self.core, &mut self.interface, keybind)
    }

    /// Run the picker interactively with a custom event source and writer.
    ///
    /// See the documentation for [`Picker::pick_with_io`](crate::Picker::pick_with_io) for more details
    /// on the behaviour.
    #[inline]
    pub fn pick_with_io<E, W>(
        mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<I::Output<'a>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: io::Write,
    {
        pick_with_io(self.core, &mut self.interface, event_source, writer)
    }

    /// Run the picker interactively with a custom event source and generic terminal backend.
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<E, W>(
        mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<I::Output<'a>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
    {
        self.interface.run(self.core, event_source, terminal)
    }
}

#[inline]
pub(crate) fn pick_with_keybind<'a, T, R, I, F>(
    core: &'a mut PickerCore<T, R>,
    interface: &mut I,
    keybind: F,
) -> Result<I::Output<'a>, PickError>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
    F: FnMut(KeyEvent) -> Option<Event>,
{
    let stderr = io::stderr().lock();
    if stderr.is_terminal() {
        pick_with_io(
            core,
            interface,
            StdinReader::new(keybind),
            &mut BufWriter::new(stderr),
        )
    } else {
        Err(PickError::NotInteractive)
    }
}

#[inline]
pub(crate) fn pick_with_io<'a, T, R, I, E, W>(
    core: &'a mut PickerCore<T, R>,
    interface: &mut I,
    event_source: E,
    writer: &mut W,
) -> Result<I::Output<'a>, PickError<E::AbortErr>>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
    E: EventSource,
    W: io::Write,
{
    interface.run(core, event_source, &mut CrosstermTerminal::new(writer))
}

impl<T: Send + Sync + 'static> Interface<T> for Single {
    type Output<'a> = Option<&'a T>;
}

impl<T: Send + Sync + 'static> sealed::State<T> for Single {
    #[inline]
    fn default_keybind(key: KeyEvent) -> Option<Event> {
        keybind_no_multi(key)
    }

    #[inline]
    fn run<'a, R, E, W>(
        &mut self,
        core: &'a mut PickerCore<T, R>,
        event_source: E,
        terminal: &mut W,
    ) -> Result<<Self as Interface<T>>::Output<'a>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: Terminal,
    {
        core.pick_impl::<_, _, (), _>(event_source, terminal, NoPreview)
    }
}

impl<T: Send + Sync + 'static> Interface<T> for Multi {
    type Output<'a> = Selection<'a, T>;
}

impl<T: Send + Sync + 'static> sealed::State<T> for Multi {
    #[inline]
    fn default_keybind(key: KeyEvent) -> Option<Event> {
        keybind_default(key)
    }

    #[inline]
    fn run<'a, R, E, W>(
        &mut self,
        core: &'a mut PickerCore<T, R>,
        event_source: E,
        terminal: &mut W,
    ) -> Result<<Self as Interface<T>>::Output<'a>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: Terminal,
    {
        core.pick_impl::<_, _, SelectedIndices, _>(event_source, terminal, NoPreview)
    }
}
