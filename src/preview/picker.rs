use crossterm::event::KeyEvent;

use super::{Preview, pane::PreviewPane};
use crate::{
    Interface, Multi, Picker, PickerCore, PickerRef, Render, Selection, Single, Terminal,
    error::PickError,
    event::{Event, EventSource},
    interface::sealed,
    match_list::SelectedIndices,
};

/// The picker [interface](Interface) which displays item previews.
///
/// The type parameter `I` is the base interface, which in practice is either [`Single`] or
/// [`Multi`]. This interface owns the previewer itself, but do note that if `P` implements
/// [`Preview`], then so does `&mut P`, which can avoid destruction of stateful previewers when
/// relevant.
///
/// In most cases, you don't need this type parameter directly: first construct a [`Picker`], then
/// call [`Picker::with_preview`].
///
/// # Preview cache
///
/// ## Standard cache
///
/// The picker stores generated previews along with scroll position in an LRU cache. This means that
/// the internal previewer may not be called to generate the preview pane if previewing already
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
///
/// ## Prefetch cache
///
/// The preview implementation `P` may decide that prefetch is worthwhile (see [`Preview::prefetch`]
/// and [`Preview::prefetch_limit`]), in which case the picker will also maintain an internal
/// prefetch cache, separate from the main cache.
pub struct WithPreview<I, P> {
    interface: I,
    previewer: P,
}

impl<P> WithPreview<Single, P> {
    /// Initialize a new preview interface in single-selection mode.
    #[must_use]
    #[inline]
    pub fn new_single(previewer: P) -> Self {
        Self {
            interface: Single,
            previewer,
        }
    }
}

impl<P> WithPreview<Multi, P> {
    /// Initialize a new preview interface in multi-selection mode.
    #[must_use]
    #[inline]
    pub fn new_multi(previewer: P) -> Self {
        Self {
            interface: Multi,
            previewer,
        }
    }
}

impl<T, R, P> Picker<T, R, WithPreview<Single, P>> {
    /// Convert into a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn multi(self) -> Picker<T, R, WithPreview<Multi, P>> {
        let (picker, previewer) = self.into_parts();
        picker
            .core
            .with_interface(WithPreview::new_multi(previewer))
    }

    /// Mutably borrow this picker as a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_multi(&mut self) -> PickerRef<'_, T, R, WithPreview<Multi, &mut P>> {
        self.core
            .as_interface(WithPreview::new_multi(&mut self.interface.previewer))
    }
}

impl<T, R, P> Picker<T, R, WithPreview<Multi, P>> {
    /// Convert back to a single-selection picker, retaining the internal previewer.
    #[must_use]
    #[inline]
    pub fn single(self) -> Picker<T, R, WithPreview<Single, P>> {
        let (picker, previewer) = self.into_parts();
        picker
            .core
            .with_interface(WithPreview::new_single(previewer))
    }

    /// Mutably borrow this picker as a single-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> PickerRef<'_, T, R, WithPreview<Single, &mut P>> {
        self.core
            .as_interface(WithPreview::new_single(&mut self.interface.previewer))
    }
}

impl<T, R, I, P> Picker<T, R, WithPreview<I, P>> {
    /// Convert into a picker without previews by dropping the previewer.
    ///
    /// This is convenient for stateless previewers, or for previewers passed by reference. To
    /// recover the internal previewer as well, use [`into_parts`](Self::into_parts).
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> Picker<T, R, I> {
        self.core.with_interface(self.interface.interface)
    }

    /// Decompose a previewing picker into the constituent picker and previewer.
    #[inline]
    pub fn into_parts(self) -> (Picker<T, R, I>, P) {
        (
            self.core.with_interface(self.interface.interface),
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

impl<'a, T, R, P> PickerRef<'a, T, R, WithPreview<Single, P>> {
    /// Convert this view type to a multi-selection variant.
    #[must_use]
    #[inline]
    pub fn multi(self) -> PickerRef<'a, T, R, WithPreview<Multi, P>> {
        let (picker, previewer) = self.into_parts();
        picker.with_interface(WithPreview::new_multi(previewer))
    }
}

impl<'a, T, R, P> PickerRef<'a, T, R, WithPreview<Multi, P>> {
    /// Convert this view type to a single-selection variant.
    #[must_use]
    #[inline]
    pub fn single(self) -> PickerRef<'a, T, R, WithPreview<Single, P>> {
        let (picker, previewer) = self.into_parts();
        picker.with_interface(WithPreview::new_single(previewer))
    }
}

impl<'a, T, R, I, P> PickerRef<'a, T, R, WithPreview<I, P>> {
    /// Convert this into a picker without previews by discarding the previewer.
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> PickerRef<'a, T, R, I> {
        PickerRef {
            core: self.core,
            interface: self.interface.interface,
        }
    }

    /// Convert this into a picker without previews, retaining the previewer.
    #[inline]
    pub fn into_parts(self) -> (PickerRef<'a, T, R, I>, P) {
        (
            PickerRef {
                core: self.core,
                interface: self.interface.interface,
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

impl<T: Send + Sync + 'static, P: Preview<T>> Interface<T> for WithPreview<Single, P> {
    type Output<'a> = Option<&'a T>;
}

impl<T: Send + Sync + 'static, P: Preview<T>> sealed::State<T> for WithPreview<Single, P> {
    #[inline]
    fn default_keybind(key: KeyEvent) -> Option<Event> {
        <Single as sealed::State<T>>::default_keybind(key)
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
        let preview = PreviewPane::new(&core.preview_config, &core.chars, &mut self.previewer);
        core.pick_impl::<_, _, (), _>(event_source, terminal, preview)
    }
}

impl<T: Send + Sync + 'static, P: Preview<T>> Interface<T> for WithPreview<Multi, P> {
    type Output<'a> = Selection<'a, T>;
}

impl<T: Send + Sync + 'static, P: Preview<T>> sealed::State<T> for WithPreview<Multi, P> {
    #[inline]
    fn default_keybind(key: KeyEvent) -> Option<Event> {
        <Multi as sealed::State<T>>::default_keybind(key)
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
        let preview = PreviewPane::new(&core.preview_config, &core.chars, &mut self.previewer);
        core.pick_impl::<_, _, SelectedIndices, _>(event_source, terminal, preview)
    }
}
