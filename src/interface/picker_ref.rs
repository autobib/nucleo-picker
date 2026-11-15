use std::io;

use crossterm::event::KeyEvent;

#[cfg(feature = "preview-cache")]
use super::{CachedPreview, Multi};
use super::{InterfaceDefaults, Single};
#[cfg(feature = "unstable-backend")]
use crate::Terminal;
use crate::{
    Driver, KeybindDefault, Render,
    error::PickError,
    event::{Event, EventSource},
    input,
    session::Interface,
};

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
    pub(crate) core: &'a mut Driver<T, R>,
    pub(crate) interface: I,
    pub(crate) defaults: InterfaceDefaults,
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
    pub fn pick(mut self) -> Result<I::Output<'a>, PickError>
    where
        I: KeybindDefault,
    {
        input::pick(self.core, &mut self.interface)
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
        input::pick_with_keybind(self.core, &mut self.interface, keybind)
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
        self.core
            .pick_with_io(&mut self.interface, event_source, writer)
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
        self.core
            .pick_with_terminal_io(&mut self.interface, event_source, terminal)
    }
}

impl<'a, T, R, I> PickerRef<'a, T, R, I> {
    /// Change the interface of this view type.
    #[must_use]
    #[inline]
    pub fn with_interface<J>(self, interface: J) -> PickerRef<'a, T, R, J> {
        PickerRef {
            core: self.core,
            interface,
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<'a, T, R, P> PickerRef<'a, T, R, CachedPreview<Single, P>> {
    /// Convert this view type to a multi-selection variant.
    #[must_use]
    #[inline]
    pub fn multi(self) -> PickerRef<'a, T, R, CachedPreview<Multi, P>> {
        PickerRef {
            interface: CachedPreview {
                interface: self.defaults.multi,
                previewer: self.interface.previewer,
                cache_size: self.interface.cache_size,
            },
            core: self.core,
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<'a, T, R, P> PickerRef<'a, T, R, CachedPreview<Multi, P>> {
    /// Convert this view type to a single-selection variant.
    #[must_use]
    #[inline]
    pub fn single(self) -> PickerRef<'a, T, R, CachedPreview<Single, P>> {
        PickerRef {
            core: self.core,
            interface: CachedPreview {
                interface: Single,
                previewer: self.interface.previewer,
                cache_size: self.interface.cache_size,
            },
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<'a, T, R, I, P> PickerRef<'a, T, R, CachedPreview<I, P>> {
    /// Convert this into a picker without previews by discarding the previewer.
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> PickerRef<'a, T, R, I> {
        PickerRef {
            core: self.core,
            interface: self.interface.interface,
            defaults: self.defaults,
        }
    }

    /// Convert this into a picker without previews, retaining the previewer.
    #[inline]
    pub fn into_parts(self) -> (PickerRef<'a, T, R, I>, P) {
        (
            PickerRef {
                core: self.core,
                interface: self.interface.interface,
                defaults: self.defaults,
            },
            self.interface.previewer,
        )
    }

    /// Returns a reference to the internal previewer.
    #[inline]
    pub fn previewer(&self) -> &P {
        &self.interface.previewer
    }

    /// Returns an exclusive reference to the internal previewer.
    #[inline]
    pub fn previewer_mut(&mut self) -> &mut P {
        &mut self.interface.previewer
    }
}
