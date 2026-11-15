#[cfg(feature = "preview-cache")]
pub(crate) mod cache;
#[cfg(feature = "preview-cache")]
pub(crate) mod controller;
mod picker_ref;
mod selection;
mod uncached;

use std::num::NonZero;
#[cfg(feature = "preview-cache")]
use std::time::Instant;

use crossterm::event::KeyEvent;

pub use picker_ref::PickerRef;
pub use selection::{MultiState, Selection, SelectionIntoIter};
pub use uncached::{UncachedPreview, UncachedPreviewState};

use crate::{
    Picker,
    event::{Event, keybind_single},
    session::{Context, Exit, Interface, PickerCore},
};
#[cfg(feature = "preview-cache")]
use crate::{
    event::keybind_passthrough_preview,
    preview::{Content, DEFAULT_CACHE_SIZE, Preview, PreviewEvent},
    session::{Dimensions, ItemId, QueueEvent, QueueStatus, Redraw},
};
#[cfg(feature = "preview-cache")]
use controller::PreviewState;

/// The single-selection picker interface.
///
/// Read the [module-level docs](crate::session) or the [interface docs](Interface) for more details.
pub struct Single;

/// The multiple-selection picker interface.
///
/// Initialize using [`Multi::new`] or the [`Default`] implementation.
///
/// Read the [module-level docs](crate::session) or the [interface docs](Interface) for more details.
#[derive(Clone, Copy, Default)]
pub struct Multi {
    pub(crate) max_selection_count: Option<NonZero<u32>>,
}

impl Multi {
    /// Initialize a new multi-selection interface with no bound on the number of selections.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            max_selection_count: None,
        }
    }

    /// The maximum number of items that can be selected in the picker.
    ///
    /// If `None`, no bound is applied; otherwise, use the provided bound. This is a `u32` since
    /// the picker cannot hold more than [`u32::MAX`] items.
    #[must_use]
    #[inline]
    pub const fn max_selection_count(mut self, maximum: Option<NonZero<u32>>) -> Self {
        self.max_selection_count = maximum;
        self
    }
}

/// The picker [interface](Interface) which displays item previews.
///
/// The type parameter `I` is any base interface. For instance, this is likely either [`Single`] or [`Multi`].
/// This interface owns the previewer itself, but do note that if `P` implements
/// [`Preview`], then so does `&mut P`, which can avoid destruction of stateful previewers when
/// relevant. The corresponding [`Interface`] implementation overrides the preview interface method
/// of the base interface and forwards the remaining calls.
///
/// The base interface `I` should probably not implement a previewer. This wrapper type will likely
/// still function correctly but will result in additional unnecessary overhead.
///
/// In most cases, you don't need this type directly: first construct a [`Picker`], then
/// call [`Picker::with_preview`].
///
/// Read the [module-level docs](crate::session) or the [interface docs](Interface) for more details.
///
/// # Preview cache
///
/// ## Standard cache
///
/// This interface stores generated previews along with scroll position in an LRU cache. This means that
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
/// prefetch cache, separate from the main cache. The prefetch cache will be 2 times the size of [`Preview::prefetch_limit`].
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub struct CachedPreview<I, P> {
    pub(crate) interface: I,
    pub(crate) previewer: P,
    pub(crate) cache_size: Option<NonZero<usize>>,
}

/// Composite state used by [`CachedPreview`].
///
/// This contains state of the base interface along with extra state used by the preview pane.
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub struct CachedPreviewState<S> {
    pub(crate) interface: S,
    pub(crate) preview: PreviewState,
}

#[cfg(feature = "preview-cache")]
impl<I, P> CachedPreview<I, P> {
    /// Wrap an interface with a new interface that generates previews.
    #[must_use]
    #[inline]
    pub fn new(interface: I, previewer: P) -> Self {
        Self {
            interface,
            previewer,
            cache_size: DEFAULT_CACHE_SIZE,
        }
    }

    /// Set the cache capacity of the main preview cache.
    ///
    /// The default value if this struct is newly created is `128`, but if this interface was constructed
    /// from picker, the default value given to [`PickerOptions::preview_cache_size`](crate::PickerOptions::preview_cache_size)
    /// is used as the default instead.
    #[must_use]
    #[inline]
    pub const fn cache_size(mut self, size: Option<NonZero<usize>>) -> Self {
        self.cache_size = size;
        self
    }

    /// Get a reference to the inner interface.
    #[must_use]
    #[inline]
    pub const fn inner(&self) -> &I {
        &self.interface
    }

    /// Get a mutable reference to the inner interface.
    #[must_use]
    #[inline]
    pub const fn inner_mut(&mut self) -> &mut I {
        &mut self.interface
    }
}

#[cfg(feature = "preview-cache")]
impl<P> CachedPreview<Single, P> {
    /// Initialize a new preview interface in single-selection mode.
    #[must_use]
    #[inline]
    pub fn new_single(previewer: P) -> Self {
        Self::new(Single, previewer)
    }
}

#[cfg(feature = "preview-cache")]
impl<P> CachedPreview<Multi, P> {
    /// Initialize a new preview interface in multi-selection mode.
    #[must_use]
    #[inline]
    pub fn new_multi(previewer: P) -> Self {
        Self::new(Multi::new(), previewer)
    }
}

impl<T: Send + Sync + 'static> Interface<T> for Single {
    type State = ();
    type Output<'a> = Option<&'a T>;

    #[inline]
    fn init(&mut self, _cx: &Context<'_, T>) {}

    #[inline]
    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Quit | Exit::Selected(None) => None,
            Exit::Selected(Some(id)) => {
                let item = cx
                    .matches()
                    .item(id)
                    .expect("selected item is not in the output context");
                Some(item.value())
            }
        }
    }

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_single(key)
    }
}

#[cfg(feature = "preview-cache")]
impl<T: Send + Sync + 'static, I: Interface<T>, P: Preview<T>> Interface<T>
    for CachedPreview<I, P>
{
    type State = CachedPreviewState<I::State>;
    type Output<'a> = I::Output<'a>;

    const HAS_PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
        CachedPreviewState {
            interface: self.interface.init(cx),
            preview: PreviewState::new(self.cache_size, self.previewer.prefetch_limit()),
        }
    }

    fn on_restart(&mut self, state: &mut Self::State, cx: &Context<'_, T>) {
        self.interface.on_restart(&mut state.interface, cx);
        state.preview.restart(&mut self.previewer);
    }

    fn on_end(&mut self, state: &mut Self::State) {
        self.interface.on_end(&mut state.interface);
        state.preview.end(&mut self.previewer);
    }

    #[inline]
    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        self.interface.is_queued(&state.interface, id)
    }

    #[inline]
    fn queue_status(&self, state: &Self::State) -> Option<QueueStatus> {
        self.interface.queue_status(&state.interface)
    }

    #[inline]
    fn on_queue(
        &mut self,
        state: &mut Self::State,
        event: QueueEvent<'_, T>,
        cx: &Context<'_, T>,
    ) -> bool {
        self.interface.on_queue(&mut state.interface, event, cx)
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        self.interface.finish(state.interface, exit, cx)
    }

    fn before_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        matches_changed: bool,
        deadline: Instant,
    ) -> Redraw {
        let mut redraw =
            self.interface
                .before_frame(&mut state.interface, cx, matches_changed, deadline);
        if state
            .preview
            .update(&mut self.previewer, cx, matches_changed, deadline)
        {
            redraw |= Redraw::PREVIEW;
        }
        redraw
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) -> Redraw {
        let redraw = self
            .interface
            .after_frame(&mut state.interface, cx, deadline);
        state
            .preview
            .update_prefetch(&mut self.previewer, cx, deadline);
        redraw
    }

    fn preview_content<'a>(&'a self, state: &'a Self::State) -> Content<'a> {
        state.preview.content()
    }

    fn preview_event(
        &mut self,
        state: &mut Self::State,
        event: PreviewEvent,
        cx: &Context<'_, T>,
    ) -> Redraw {
        if state
            .preview
            .handle_event(cx.hovered().map(|item| item.id().index), event)
        {
            Redraw::PREVIEW
        } else {
            Redraw::NONE
        }
    }

    fn preview_resize(&mut self, state: &mut Self::State, size: Option<Dimensions>) {
        state.preview.resize(size);
    }

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key).or_else(|| keybind_passthrough_preview(key).ok())
    }
}

impl<T, R> PickerCore<T, R> {
    #[inline]
    pub(crate) fn with_interface<I>(self, interface: I) -> Picker<T, R, I> {
        Picker {
            core: self,
            interface,
        }
    }

    #[inline]
    pub(crate) fn as_interface<I>(&mut self, interface: I) -> PickerRef<'_, T, R, I> {
        PickerRef {
            core: self,
            interface,
        }
    }
}
