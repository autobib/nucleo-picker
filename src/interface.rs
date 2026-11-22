//! # Interactive interface
//!
//! The *picker interface* describes the behaviour of the picker during interactive operation.
//! The interface is the type parameter `I` in the definition of the
//! [`Picker`] struct. The [`Interface`] governs the interactive behaviour of the picker (so that
//! you only pay for interface machinery you actually use) as well as the final return type.
//!
//! The main entrypoint is the [`Interface`] trait. However, you probably don't need to implement it
//! yourself; you can typically use one of the built-in interfaces defined in this module
//! (see [`Single`], [`Multi`], [`CachedPreview`], and [`UncachedPreview`]).
//!
//! ## Interfaces
//!
//! In order to choose an interface, start with [`Picker::new`] for single-selection.
//! Use [`Picker::multi`] (or [`Picker::as_multi`]) to change to multiple-selection mode:
//!
//! ```no_run
//! use nucleo_picker::{Picker, render::StrRenderer};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//!
//! if let Some(color) = picker.pick()? {
//!     println!("Selected {color}");
//! }
//!
//! for color in picker.as_multi().pick()? {
//!     println!("Selected {color}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Independently of the interface, the same three `pick*` methods are always available.
//!
//! | Interface | How to get there | Successful return value |
//! | --- | --- | --- |
//! | [`Single`] (default) | `Picker::new(renderer)` | `Option<&T>` |
//! | [`Multi`] | `picker.multi()` | [`Selection<'_, T>`] |
//! | [`CachedPreview<Single, P>`] | `picker.with_preview(previewer)` | `Option<&T>` |
//! | [`CachedPreview<Multi, P>`] | `picker.multi().with_preview(previewer)` | [`Selection<'_, T>`] |
//!
//! Note that the built-in preview interface requires the `preview-cache` feature. When the picker quits without a
//! selection, it returns either `None` or an empty [`Selection`]. Also note the built-in [`UncachedPreview`] interface.
//!
//! ### Multiple selections
//!
//! With the [`Multi`] or [`CachedPreview<Multi, P>`] interface, the interactive picker permits the
//! user to make multiple selections.
//!
//! The `pick*` methods are analogous to the single-selection variants, except additional items can be
//! queued with the [`MatchListEvent::ToggleDown`](crate::event::MatchListEvent::ToggleDown) and
//! [`MatchListEvent::ToggleUp`](crate::event::MatchListEvent::ToggleUp) events. The default
//! keybindings bind these to `⇥` and `shift + ⇥` respectively. In this case, an
//! [`Event::Select`] is handled slightly differently: if there are no queued selections, this
//! picks the hovered item, but if there are queued selections, then only the queued selections
//! are returned.
//!
//! If the picker [restarts while running](Event::Restart), the queued item list will be cleared
//! since the previous items are removed.
//!
//! The selected items are returned as a [`Selection`], which is empty if picker exited with
//! [`Event::Quit`] (or [`Event::QuitPromptEmpty`]), and non-empty if not.
//!
//! ### Adding previews
//!
//! With the `preview-cache` feature enabled, [`Picker::with_preview`] wraps the current selection mode in
//! a [`CachedPreview<I, P>`]. Converting between single and multiple selection preserves the
//! previewer. See [`crate::preview`] for constructing a previewer.
//!
//! The corresponding pick methods on the resulting [`Picker`] have same behaviour, except
//! with an additional preview pane rendered on the right hand side of the screen.
//! There is also additional handling of events which target the
//! preview pane in particular.
//!
//! The interface itself owns the previewer `P`, but do note that one can pass `&mut P` to borrow an
//! existing previewer if it is useful for the previewer state to outlive the session.
//!
//! ```no_run
//! # fn main() -> std::io::Result<()> {
//! # #[cfg(feature = "preview-cache")]
//! # {
//! use nucleo_picker::{Picker, interface::CachedPreview, preview::{Pane, SyncPreviewer}, render::StrRenderer};
//!
//! let mut picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//! let mut previewer = SyncPreviewer(|color: &&str, pane: &mut Pane| {
//!     pane.buffer_mut().push_text(color);
//! });
//!
//! let selected = picker
//!     .as_interface(CachedPreview::new_single(&mut previewer))
//!     .pick()?;
//! # }
//! # Ok(())
//! # }
//! ```
//!
//! For an owned preview-enabled picker, [`Picker::into_parts`] recovers both the picker and
//! its previewer. [`Picker::without_preview`] discards the previewer instead.
//!
//! ### Emulate single selection using a multi-picker
//!
//! It is possible to emulate single selection with multi-picker methods by setting
//! [`PickerOptions::max_selection_count`](crate::PickerOptions::max_selection_count) to `NonZero::new(1)`. This will force the resulting
//! [`Selection`] to contain either 0 or 1 element, and you can convert to `Option<&T>` by calling
//! `next` on the [iterator](Selection::iter).
//!
//! Note that the picker interface will be slightly different: it is still possible to
//! queue at most one picked item using `⇥`. With the single-selection pickers, it is not
//! possible to queue items at all.
//!
//! ## Changing the interface
//!
//! Owned conversions such as [`Picker::multi`] and [`Picker::single`] consume the picker and
//! return a picker (with the same internal items, renderer, query, etc.) with the new interface.
//! In particular, interface conversions are quite cheap in general. This form is most convenient when all subsequent sessions
//! use the same interface, or as part of the builder initialization starting with
//! [`PickerOptions`](crate::PickerOptions).
//!
//! ```no_run
//! # use nucleo_picker::{Picker, render::StrRenderer};
//! # fn main() -> std::io::Result<()> {
//! let picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//! let mut picker = picker.multi();
//! let selected = picker.pick()?;
//! # Ok(())
//! # }
//! ```
//!
//! It is also possible to temporarily change the interface using borrowing conversions which return
//! a [`PickerRef`]. These methods are named like `as_*`, such as [`Picker::as_multi`] and [`Picker::as_single`].
//! A [`PickerRef`] view mutably borrows the original picker's state and its `pick*` methods consume
//! the view. The original picker's interface is preserved and the query and item changes during
//! the session affect the original picker. The selection returned by a [`PickerRef`] borrows
//! only from the original picker.
//!
//! ```no_run
//! use nucleo_picker::{Picker, error::PickError, render::StrRenderer};
//!
//! # fn main() -> Result<(), PickError> {
//! let mut picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//!
//! // borrow as a multi-selection picker without consuming
//! // the original picker. the selection lifetime is independent
//! // of the view type
//! for color in picker.as_multi().pick()? {
//!     println!("{color}");
//! }
//!
//! // the original picker is still a single-selection picker
//! if let Some(color) = picker.pick()? {
//!     println!("{color}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! In order to explicitly choose an interface, use [`Picker::with_interface`] or [`Picker::as_interface`].
//!
//! ## Picker generics
//!
//! In most cases, the picker generic parameters can be inferred from the renderer, inserted items,
//! and the conversion methods directly. Regardless, here is an overview of the type parameters of a
//! [`Picker<T, R, I>`] and [`PickerRef<'a, T, R, I>`]:
//!
//! - `T` is the *item type*. Running the picker requires
//!   `T: Send + Sync + 'static`. Selected items are borrowed from the picker.
//! - `R` is the *renderer*, implementing [`Render<T>`](crate::Render).
//! - `I` is the *session interface*, defaulting to [`Single`]. Interactive methods require
//!   `I: Interface<T>`. Headless methods such as [`Picker::filter`] are independent of the
//!   interface.
//! - `'a` in `PickerRef<'a, T, R, I>` is the lifetime of the mutable borrow of the
//!   original picker. Its `pick*` methods return `I::Output<'a>`.
//!
//! In most cases you can use one of the provided interfaces. To customize how
//! items are displayed, implement [`Render`](crate::Render) or [`Preview`]; to customize IO, use
//! [`Picker::pick_with_keybind`] or [`Picker::pick_with_io`].
//!
//! ### Writing code which is generic over a picker
//!
//! The [`Interface`] trait also helps to write code which is generic over any picker. The
//! associated [`Interface::Output<'a>`] type implements [`IntoIterator`], with the resulting
//! iterator being an [`ExactSizeIterator`].
//!
//! ```no_run
//! use nucleo_picker::{Picker, Render, error::PickError, event::KeybindDefault, interface::Interface};
//!
//! fn print_selection<T, R, I>(picker: &mut Picker<T, R, I>) -> Result<(), PickError>
//! where
//!     T: Send + Sync + 'static + std::fmt::Display,
//!     R: Render<T>,
//!     I: Interface<T> + KeybindDefault,
//! {
//!     for item in picker.pick()? {
//!         println!("{item}");
//!     }
//!     Ok(())
//! }
//! ```

#[cfg(feature = "preview-cache")]
pub(crate) mod cache;
#[cfg(feature = "preview-cache")]
pub(crate) mod controller;
mod picker_ref;
mod selection;
mod uncached;

use std::{num::NonZero, time::Instant};

use crossterm::event::KeyEvent;

pub use picker_ref::PickerRef;
pub use selection::{MultiState, Selection, SelectionIntoIter};
pub use uncached::{UncachedPreview, UncachedPreviewState};

#[cfg(doc)]
use crate::Picker;
use crate::{
    event::{Event, KeybindDefault},
    keybind::keybind_single,
    preview::{Content, PreviewEvent},
    session::{Components, Context, Dimensions, Exit, ItemId, QueueEvent, QueueStatus},
};
#[cfg(feature = "preview-cache")]
use crate::{
    keybind::keybind_passthrough_preview,
    preview::{DEFAULT_CACHE_SIZE, Preview},
};
#[cfg(feature = "preview-cache")]
use controller::PreviewState;

/// Generics for the picker interface.
///
/// This trait constrains the type parameter `I` of [`Picker`] when used interactively.
/// Morally, the [`Interface`] defines *the behaviour of the interactive picker*. This includes:
///
/// 1. [`Single`]-selection and [`Multi`]-selection modes, which allows queuing selections in the
///    interactive interface and sets the return type (the `Output` type parameter) as
///    [`Option<&T>`] or [`Selection<'_, T>`] respectively.
/// 2. Preview-enabled pickers (via [`CachedPreview`]) which render item
///    previews in a separate pane.
///
/// Read the [module documentation](crate::session) for an overview and more details. For details on
/// how this trait is called by default, see the [picker interface call order
/// documentation](Picker#interface-call-order).
pub trait Interface<T>: Sized {
    /// Arbitrary state which persists for exactly one session.
    type State;

    /// Whether this interface provides a preview pane.
    const PREVIEW: bool = false;

    /// The output of the picker after selection.
    type Output<'a>: IntoIterator<Item = &'a T, IntoIter: ExactSizeIterator>
    where
        T: 'a;

    /// Initialize the state after initial match-list updates.
    ///
    /// The provided [`Context`] is the shared first frame context which is also passed to the call
    /// to [`before_frame`](Self::before_frame) which occurs during initialization.
    ///
    /// Depending on the runner, initialization may or not may see up-to-date match items.
    fn init(&mut self, cx: &Context<'_, T>) -> Self::State;

    /// Process a restart event.
    ///
    /// The provided [`Context`] corresponds to the *previous* generation at the moment that the
    /// restart event was processed.
    fn on_restart(&mut self, _state: &mut Self::State, _cx: &Context<'_, T>) {}

    /// Perform cleanup at the end of the session.
    ///
    /// This takes place before converting output in [`finish`](Self::finish) and also before
    /// propagating errors.
    fn on_end(&mut self, _state: &mut Self::State) {}

    /// Returns if a given identifier is a queued item.
    fn is_queued(&self, _state: &Self::State, _id: ItemId) -> bool {
        false
    }

    /// The current number of queued items and the limit, if queueing is supported.
    ///
    /// `None` means queueing is unsupported by this interface. This is used to render the marker
    /// in the status line.
    fn queue_status(&self, _state: &Self::State) -> Option<QueueStatus> {
        None
    }

    /// Process an event which modifies the item queue, returning whether or not there were changes
    /// to the queue.
    fn on_queue(
        &mut self,
        _state: &mut Self::State,
        _event: QueueEvent<'_, T>,
        _cx: &Context<'_, T>,
    ) -> bool {
        false
    }

    /// Generate the output on completion.
    ///
    /// This method is called after [`on_end`](Self::on_end) when handling one of the standard [exit types](Exit),
    /// including selection or quit.
    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items>;

    /// Perform work before rendering the frame, after processing matches and navigation.
    ///
    /// This method is called on every frame, regardless of whether changes occurred or not. The
    /// resulting redraw mask influences the subsequent screen redraw.
    fn before_frame(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _matches_changed: bool,
        _deadline: Instant,
    ) -> Components {
        Components::NONE
    }

    /// Perform work after rendering the frame.
    // TODO: add more details here, including on the frame geometry (?)
    fn after_frame(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _deadline: Instant,
    ) -> Components {
        Components::NONE
    }

    /// Return the current preview pane.
    ///
    /// If the preview pane is [enabled](Self::PREVIEW), this is usually the rendered preview pane corresponding to the current item. If the preview pane is not enabled, this value is ignored and the implementation should just return [`Content::Empty`]. This is the default value.
    fn preview_content<'a>(&'a self, _state: &'a Self::State) -> Content<'a> {
        Content::Empty
    }

    /// Handle a preview event.
    ///
    /// The session forwards preview events whenever [`Self::PREVIEW`] is true, including
    /// while hidden. Hosts decide whether to filter user input; the built-in runner
    /// discards preview events while hidden. [`Context::preview_enabled`] reports the
    /// current visibility independently of event delivery.
    /// The returned [`Components`] is a mask referring to the subset of the screen which requires
    /// redraw.
    fn preview_event(
        &mut self,
        _state: &mut Self::State,
        _event: PreviewEvent,
        _cx: &Context<'_, T>,
    ) -> Components {
        Components::NONE
    }

    /// Update the dimensions of the preview pane.
    ///
    /// These are the dimensions of the drawable area of the preview pane (including width and
    /// height 0 if the preview pane if the preview pane is very small), or `None` if the preview
    /// pane is hidden.
    ///
    /// This callback updates preview presentation. The host repaints after resizing; the
    /// session does not invalidate components merely because their dimensions change.
    /// Changes to other components must be reported through the interface's invalidation hooks.
    fn preview_resize(&mut self, _state: &mut Self::State, _size: Option<Dimensions>) {}
}

#[derive(Clone, Copy)]
pub(crate) struct InterfaceDefaults {
    pub(crate) multi: Multi,
    #[cfg(feature = "preview-cache")]
    pub(crate) preview_cache_size: Option<NonZero<usize>>,
}

impl InterfaceDefaults {
    pub(crate) const fn new() -> Self {
        Self {
            multi: Multi::new(),
            #[cfg(feature = "preview-cache")]
            preview_cache_size: DEFAULT_CACHE_SIZE,
        }
    }
}

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
/// automatically. The built-in runner ignores preview-related events (such as pane movement and
/// refresh) while hidden. Hosts driving a [`Session`](crate::session::Session) directly can still
/// submit preview events, for example to invalidate cached content before revealing the pane.
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
}

impl KeybindDefault for Single {
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

#[cfg(feature = "preview-cache")]
impl<I: KeybindDefault, P> KeybindDefault for CachedPreview<I, P> {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key).or_else(|| keybind_passthrough_preview(key).ok())
    }
}
