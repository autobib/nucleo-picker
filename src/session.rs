//! # The picker session
//!
//! A *session* is a single interactive call to one of the core picker methods: [`Picker::pick`],
//! [`Picker::pick_with_keybind`], and [`Picker::pick_with_io`]. The behaviour in a session is
//! determined by the [`Interface`] of the picker (the type parameter `I` in the definition of the
//! [`Picker`] struct). The [`Interface`] governs the interactive behaviour of the session (so that
//! you only pay for interface machinery you actully use) as well as the final return type.
//!
//! The picker can be re-used for multiple sessions: the items and query are retained between
//! sessions (but can be cleared by [restarting](Picker::restart)).
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
//! Independently of the interface, the same three `pick*` methods are always available:
//!
//! | Interface | How to get there | Successful return value |
//! | --- | --- | --- |
//! | [`Single`] (default) | `Picker::new(renderer)` | `Option<&T>` |
//! | [`Multi`] | `picker.multi()` | [`Selection<'_, T>`] |
//! | [`WithPreview<Single, P>`] | `picker.with_preview(previewer)` | `Option<&T>` |
//! | [`WithPreview<Multi, P>`] | `picker.multi().with_preview(previewer)` | [`Selection<'_, T>`] |
//!
//! Note that the preview interfaces require the `preview` feature. When the picker quits without a
//! selection, it returns either `None` or an empty [`Selection`].
//!
//! ### Multiple selections
//!
//! With the [`Multi`] or [`WithPreview<Multi, P>`] interface, the interactive picker permits the
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
//! With the `preview` feature enabled, [`Picker::with_preview`] wraps the current selection mode in
//! a [`WithPreview<I, P>`]. Converting between single and multiple selection preserves the
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
//! # #[cfg(feature = "preview")]
//! # {
//! use nucleo_picker::{Picker, WithPreview, preview::{Pane, SyncPreviewer}, render::StrRenderer};
//!
//! let mut picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//! let mut previewer = SyncPreviewer(|color: &&str, pane: &mut Pane| {
//!     pane.buffer_mut().push_text(color);
//! });
//!
//! let selected = picker
//!     .as_interface(WithPreview::new_single(&mut previewer))
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
//! In particular, interface conversions are zero-cost. This form is most convenient when all subsequent sessions
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
//! the view. The original picker's interface is preserved and the query and item changes duriong
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
//! In most cases, the picker generic paramters can be inferred from the renderer, inserted items,
//! and the conversion methods directly. Regardless, here is an overview of the type parameters of a
//! [`Picker<T, R, I>`] and [`PickerRef<'a, T, R, I>`]:
//!
//! - `T` is the *item type*. Running the picker requires
//!   `T: Send + Sync + 'static`. Selected items are borrowed from the picker.
//! - `R` is the *renderer*, implementing [`Render<T>`].
//! - `I` is the *session interface*, defaulting to [`Single`]. Interactive methods require
//!   `I: Interface<T>`. Headless methods such as [`Picker::filter`] are independent of the
//!   interface.
//! - `'a` in `PickerRef<'a, T, R, I>` is the lifetime of the mutable borrow of the
//!   original picker. Its `pick*` methods return `I::Output<'a>`.
//!
//! In most cases you can use one of the provided interfaces. To customize how
//! items are displayed, implement [`Render`] or [`Preview`]; to customize IO, use
//! [`Picker::pick_with_keybind`] or [`Picker::pick_with_io`].
//!
//! ### Writing code which is generic over a picker
//!
//! The [`Interface`] trait also helps to write code which is generic over any picker. The
//! associated [`Interface::Output<'a>`] type implements [`IntoIterator`], with the resulting
//! iterator being an [`ExactSizeIterator`].
//!
//! ```no_run
//! use nucleo_picker::{Interface, Picker, Render, error::PickError};
//!
//! fn print_selection<T, R, I>(picker: &mut Picker<T, R, I>) -> Result<(), PickError>
//! where
//!     T: Send + Sync + 'static + std::fmt::Display,
//!     R: Render<T>,
//!     I: Interface<T>,
//! {
//!     for item in picker.pick()? {
//!         println!("{item}");
//!     }
//!     Ok(())
//! }
//! ```

mod context;
mod item;
mod matches;
mod picker_ref;
mod redraw;
mod run;
#[cfg(test)]
mod tests;

use std::{
    io::{self, BufWriter, IsTerminal},
    num::NonZero,
    time::Instant,
};

use crossterm::event::KeyEvent;

pub use crate::match_list::{MultiState, QueueEvent};
pub use crate::rect::Dimensions;
pub use context::Context;
pub use item::{ItemId, ItemRef, OwnedItem};
pub use matches::Matches;
pub use picker_ref::PickerRef;
pub use redraw::Redraw;

#[cfg(doc)]
use crate::{Picker, Selection};
use crate::{
    PickerCore, Render,
    error::PickError,
    event::{Event, EventSource, StdinReader, keybind_single},
    terminal::CrosstermTerminal,
};
#[cfg(feature = "preview")]
use crate::{
    event::keybind_passthrough_preview,
    preview::{Content, DEFAULT_CACHE_SIZE, Preview, PreviewEvent, controller::PreviewState},
};

/// Generics for the picker interface.
///
/// This trait constrains the type parameter `I` of [`Picker`] when used interactively.
/// Morally, the [`Interface`] defines *the behaviour of the interactive picker*. This includes:
///
/// 1. [`Single`]-selection and [`Multi`]-selection modes, which allows queuing selections in the
///    interactive interface and sets the return type (the `Output` type parameter) as
///    [`Option<&T>`] or [`Selection<'_, T>`] respectively.
/// 2. Preview-enabled pickers (via [`WithPreview`]) which render item
///    previews in a separate pane.
///
/// Read the [module documentation](self) for an overview and more details.
///
/// # Interface call order
///
/// The [`Picker`] drives an interface implementation in order to produce the interactive picker.
/// The picker drives the loop until [completion](#completion-and-errors) (i.e. if a selection is
/// made, the picker quits, or there is an error).
///
/// ## Initialization
///
/// When the picker starts up, the following call sequence is made.
///
/// 1. [`init`](Self::init) is called once with context containing the query and match snapshot,
///    with no hovered item. The preview pane is enabled according to `HAS_PREVIEW` and is
///    dimensionless.
/// 2. The match engine is updated with any changes which were processed in the background and the
///    terminal is setup.
/// 3. Terminal dimensions are queried and resizes are propagated, for instance via
///    [`preview_resize`](Self::preview_resize) if the preview pane is enabled.
/// 4. [`before_frame`](Self::before_frame) is called with the initial hovered item.
/// 5. [`queued_count`](Self::queued_count) and [`queue_limit`](Self::queue_limit) are called to
///    update the status line.
/// 6. The first full frame is drawn.
/// 7. [`after_frame`](Self::after_frame) is called. The draw mask is used for the next frame.
///
/// ## Frame loop
///
/// After a batch ends without exiting:
///
/// 1. A bounded batch of events is processed. Events which cause the picker to exit are immediately
///    processed. This updates the prompt and match list, reparses if the query changed, and updates
///    the match engine.
/// 2. [`before_frame`](Self::before_frame) is called.
/// 3. [`queued_count`](Self::queued_count) and [`queue_limit`](Self::queue_limit) are called to
///    update the status line.
/// 4. Check for pending redraws either from events or as required by the interface. If a resize is
///    required, a full redraw is done. If [`HAS_PREVIEW`](Self::HAS_PREVIEW) is true, the new size
///    [`preview_resize`](Self::preview_resize) with the new dimensions.
/// 5. Draw the components.
/// 6. If a status request can be processed, call [`queued_count`](Self::queued_count).
/// 7. [`after_frame`](Self::after_frame) is called. Its redraw mask is carried over to the next
///    frame.
///
/// Note that the frame hooks ([`before_frame`](Self::before_frame) and
/// [`after_frame`](Self::after_frame)) are run on all frames, independently of whether or not
/// changes were made.
///
/// ## Completion and errors
///
/// After an exit or error while processing events, the driver records any changes in the underlying
/// picker, attempts to clean up the terminal, and then calls [`on_end`](Self::on_end). If the exit
/// and cleanup succeeds, [`finish`](Self::finish) is called.
///
/// Abnormal exit (for instance with IO errors) will still result in attempted terminal cleanup but
/// will not result in usual interface cleanup methods from being called.
pub trait Interface<T>: Sized {
    /// Arbitrary state which persists for exactly one session.
    type State;

    /// Whether this interface provides a preview pane.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    const HAS_PREVIEW: bool = false;

    /// The output of the picker after selection. This is an [`Option<&T>`] for single-selection
    /// pickers and [`Selection<'_, T>`] for multi-selection pickers.
    type Output<'a>: IntoIterator<Item = &'a T, IntoIter: ExactSizeIterator>
    where
        T: 'a;

    /// Initialize the state before any matching and layout computations.
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

    /// The current number of queued items.
    ///
    /// `None` means queueing is disabled entirely; `Some(n)` indicates the number of queued
    /// items.
    fn queued_count(&self, _state: &Self::State) -> Option<usize> {
        None
    }

    /// The maximum number of queued items permitted by this interface, or `None` if unlimited.
    ///
    /// This is used to render the marker in the status line.
    fn queue_limit(&self, _state: &Self::State) -> Option<NonZero<u32>> {
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
    ) -> Redraw {
        Redraw::NONE
    }

    /// Perform work after rendering the frame.
    // TODO: add more details here, including on the frame geometry (?)
    fn after_frame(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _deadline: Instant,
    ) -> Redraw {
        Redraw::NONE
    }

    /// Return the current preview pane.
    ///
    /// If the preview pane is [enabled](Self::HAS_PREVIEW), this is usually the rendered preview pane corresponding to the current item. If the preview pane is not enabled, this value is ignored and the implementation should just return [`Content::Empty`]. This is the default value.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    fn preview_content<'a>(&'a self, _state: &'a Self::State) -> Content<'a> {
        Content::Empty
    }

    /// Handle a preview event.
    ///
    /// The preview event is only forwarded if the preview pane is [enabled](Self::HAS_PREVIEW).
    /// Use the returned [`Redraw`] mask if the screen needs to be redrawn.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    fn preview_event(
        &mut self,
        _state: &mut Self::State,
        _event: PreviewEvent,
        _cx: &Context<'_, T>,
    ) -> Redraw {
        Redraw::NONE
    }

    /// Update the dimensions of the preview pane.
    ///
    /// These are the dimensions of the drawable area of the preview pane, or `None` if the preview
    /// pane is hidden.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    fn preview_resize(&mut self, _state: &mut Self::State, _size: Option<Dimensions>) {}

    /// The default keybindings for this interface.
    ///
    /// This method is generic over the abort type `A` for compatibility with arbitrary application
    /// errors. Interfaces use this to specialize event handling to only emit events which this
    /// specific interface requires. See the
    /// [keybind specialization documentation](crate::event::keybind_default#specialized-bindings)
    /// for more details.
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        crate::event::keybind_default(key)
    }
}

/// The status on successful session exit.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// The session quit without making a selection.
    Quit,
    /// The session quit and requested a selection. The given item, if any, is the hovered at the time of
    /// processing the exit.
    Selected(Option<ItemId>),
}

/// Matching progress after matching updates.
///
/// This is returned by [`Picker::update_matches`](crate::Picker::update_matches). The counts display up-to-date information
/// as available with [`Picker::matches`](crate::Picker::matches). Use [`is_complete`](Self::is_complete) for a convenience
/// check that matching is complete.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchStatus {
    /// Whether there were any changes to the [`Matches`].
    pub items_changed: bool,
    /// The number of items in the [`Matches`]
    pub matched_item_count: u32,
    /// The total number of match candidates.
    pub item_count: u32,
    /// Whether there is matching work to be done.
    pub matching: bool,
    /// Whether there are any remaining live injectors.
    pub injecting: bool,
}

impl MatchStatus {
    /// Returns if all input has been received and match work has been done.
    ///
    /// This is `!self.matching && !self.injecting`: in other words, all input must be
    /// received and all matching work must be complete against the current query.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.matching && !self.injecting
    }
}

/// The single-selection picker interface.
///
/// Read the [module-level docs](self) or the [interface docs](Interface) for more details.
pub struct Single;

/// The multiple-selection picker interface.
///
/// Initialize using [`Multi::new`] or the [`Default`] implementation.
///
/// Read the [module-level docs](self) or the [interface docs](Interface) for more details.
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
/// The type parameter `I` is the base interface, which in practice is either [`Single`] or
/// [`Multi`]. This interface owns the previewer itself, but do note that if `P` implements
/// [`Preview`], then so does `&mut P`, which can avoid destruction of stateful previewers when
/// relevant. The corresponding [`Interface`] implementation overrides the preview interface method
/// of the base interface and forwards the remaining calls.
///
/// In most cases, you don't need this type directly: first construct a [`Picker`], then
/// call [`Picker::with_preview`].
///
/// Read the [module-level docs](self) or the [interface docs](Interface) for more details.
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
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
pub struct WithPreview<I, P> {
    pub(crate) interface: I,
    pub(crate) previewer: P,
    pub(crate) cache_size: Option<NonZero<usize>>,
}

/// Composite state used by [`WithPreview`].
///
/// This contains state of the base interface along with extra state used by the preview pane.
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
pub struct WithPreviewState<S> {
    pub(crate) interface: S,
    pub(crate) preview: PreviewState,
}

#[cfg(feature = "preview")]
impl<I, P> WithPreview<I, P> {
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
}

#[cfg(feature = "preview")]
impl<P> WithPreview<Single, P> {
    /// Initialize a new preview interface in single-selection mode.
    #[must_use]
    #[inline]
    pub fn new_single(previewer: P) -> Self {
        Self::new(Single, previewer)
    }
}

#[cfg(feature = "preview")]
impl<P> WithPreview<Multi, P> {
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

#[cfg(feature = "preview")]
impl<T: Send + Sync + 'static, I: Interface<T>, P: Preview<T>> Interface<T> for WithPreview<I, P> {
    type State = WithPreviewState<I::State>;
    type Output<'a> = I::Output<'a>;

    const HAS_PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, T>) -> Self::State {
        assert!(
            !I::HAS_PREVIEW,
            "preview adapters require a preview-free base"
        );
        WithPreviewState {
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
    fn queued_count(&self, state: &Self::State) -> Option<usize> {
        self.interface.queued_count(&state.interface)
    }

    #[inline]
    fn queue_limit(&self, state: &Self::State) -> Option<NonZero<u32>> {
        self.interface.queue_limit(&state.interface)
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
    core.pick_impl(event_source, &mut CrosstermTerminal::new(writer), interface)
}
