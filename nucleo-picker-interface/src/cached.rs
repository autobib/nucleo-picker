use std::{num::NonZero, time::Instant};

use nucleo_picker_core::{
    event::PreviewEvent,
    preview::Content,
    session::{Components, Context, Dimensions, Exit, ItemId, QueueEvent, QueueStatus},
};

use crate::{
    Interface, Multi, Single,
    preview::{Preview, PreviewState},
};

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
/// Read the [interface docs](Interface) for more context.
///
/// # Preview cache
///
/// ## Standard cache
///
/// This interface stores generated previews along with scroll position in an LRU cache. This means that
/// the internal previewer may not be called to generate the preview pane if previewing already
/// succeeded earlier. The cache is cleared when processing a [restart event](Interface::on_restart).
/// See the [`Preview`] docs for more detail.
///
/// The buffers are reused within a given session and are dropped on exit or restart.
///
/// When the preview is hidden, the cache is preserved
/// and pending requests continue to be handled, but no new preview requests will be made
/// automatically. The built-in runner ignores preview-related events (such as pane movement and
/// refresh) while hidden. Hosts driving a [`Session`](nucleo_picker_core::session::Session) directly can still
/// submit preview events, for example to invalidate cached content before revealing the pane.
///
/// ## Prefetch cache
///
/// The preview implementation `P` may decide that prefetch is worthwhile (see [`Preview::prefetch`]
/// and [`Preview::prefetch_limit`]), in which case the picker will also maintain an internal
/// prefetch cache, separate from the main cache. The prefetch cache will be 2 times the size of [`Preview::prefetch_limit`].
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub struct CachedPreview<I, P> {
    interface: I,
    previewer: P,
    cache_size: Option<NonZero<usize>>,
}

/// Composite state used by [`CachedPreview`].
///
/// This contains state of the base interface along with extra state used by the preview pane.
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub struct CachedPreviewState<S> {
    interface: S,
    preview: PreviewState,
}

impl<I, P> CachedPreview<I, P> {
    /// The default main preview cache capacity.
    pub const DEFAULT_CACHE_SIZE: Option<NonZero<usize>> = NonZero::new(128);

    /// Wrap an interface with a new interface that generates previews.
    #[must_use]
    #[inline]
    pub fn new(interface: I, previewer: P) -> Self {
        Self {
            interface,
            previewer,
            cache_size: Self::DEFAULT_CACHE_SIZE,
        }
    }

    /// Set the cache capacity of the main preview cache, or `None` for an unbounded preview cache.
    ///
    /// The default value is `128`.
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

    /// Get a reference to the previewer.
    #[must_use]
    #[inline]
    pub const fn previewer(&self) -> &P {
        &self.previewer
    }

    /// Get a mutable reference to the previewer.
    #[must_use]
    #[inline]
    pub const fn previewer_mut(&mut self) -> &mut P {
        &mut self.previewer
    }

    /// Recover the inner interface and previewer, discarding configuration.
    #[inline]
    pub fn into_parts(self) -> (I, P) {
        (self.interface, self.previewer)
    }

    /// Set a new inner interface, retaining the previewer and configuration.
    #[must_use]
    #[inline]
    pub fn with_interface<J>(self, interface: J) -> CachedPreview<J, P> {
        CachedPreview {
            interface,
            previewer: self.previewer,
            cache_size: self.cache_size,
        }
    }

    /// Borrow the preview with a temporary replacement interface.
    ///
    /// Configuration from the original previewer is retained.
    #[must_use]
    #[inline]
    pub const fn as_interface<J>(&mut self, interface: J) -> CachedPreview<J, &mut P> {
        CachedPreview {
            interface,
            previewer: &mut self.previewer,
            cache_size: self.cache_size,
        }
    }
}

impl<P> CachedPreview<Single, P> {
    /// Initialize a new preview interface in single-selection mode.
    #[must_use]
    #[inline]
    pub fn new_single(previewer: P) -> Self {
        Self::new(Single, previewer)
    }
}

impl<P> CachedPreview<Multi, P> {
    /// Initialize a new preview interface in multi-selection mode.
    #[must_use]
    #[inline]
    pub fn new_multi(previewer: P) -> Self {
        Self::new(Multi::new(), previewer)
    }
}

impl<T: Send + Sync + 'static, I: Interface<T>, P: Preview<T>> Interface<T>
    for CachedPreview<I, P>
{
    type State = CachedPreviewState<I::State>;
    type Output<'a> = I::Output<'a>;

    const PREVIEW: bool = true;

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
    ) -> Components {
        let mut redraw =
            self.interface
                .before_frame(&mut state.interface, cx, matches_changed, deadline);
        if state
            .preview
            .update(&mut self.previewer, cx, matches_changed, deadline)
        {
            redraw |= Components::PREVIEW;
        }
        redraw
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, T>,
        deadline: Instant,
    ) -> Components {
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
    ) -> Components {
        if state
            .preview
            .handle_event(cx.hovered().map(|item| item.id()), event)
        {
            Components::PREVIEW
        } else {
            Components::NONE
        }
    }

    fn preview_resize(&mut self, state: &mut Self::State, size: Option<Dimensions>) {
        state.preview.resize(size);
    }
}
