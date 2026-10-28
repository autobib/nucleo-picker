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
//! picks the highlighted item, but if there are queued selections, then only the queued selections
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
//! use nucleo_picker::{Picker, WithPreview, preview::{PreviewBuffer, SyncPreviewer}, render::StrRenderer};
//!
//! let mut picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//! let mut previewer = SyncPreviewer(|color: &&str, buffer: &mut PreviewBuffer| {
//!     buffer.push_text(color);
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
//! The [`Interface`] trait is sealed: use one of the provided interfaces. To customize how
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
mod picker_ref;
mod run;
#[cfg(test)]
mod tests;

use std::io::{self, BufWriter, IsTerminal};

use crossterm::event::KeyEvent;

pub(crate) use context::Context;
pub use item::{ItemId, ItemRef};
pub use picker_ref::PickerRef;

#[cfg(doc)]
use crate::Picker;
use crate::{
    PickerChars, PickerCore, Render, Selection, Terminal,
    component::{NoPreview, PreviewComponent},
    error::PickError,
    event::{Event, EventSource, StdinReader, keybind_multi, keybind_single},
    match_list::SelectedIndices,
    terminal::CrosstermTerminal,
};
#[cfg(feature = "preview")]
use crate::{
    event::{keybind_multi_preview, keybind_single_preview},
    preview::{Preview, PreviewConfig, pane::PreviewPane},
};

mod sealed {
    use super::*;

    pub(crate) trait State<T>: Sized {
        type Queued: Default
            + for<'a> crate::match_list::Queued<Output<'a, T> = <Self as Interface<T>>::Output<'a>>
        where
            T: Send + Sync + 'static,
            Self: Interface<T>;

        type Preview<'a>: PreviewComponent<T>
        where
            Self: 'a;

        fn make_preview<'a>(
            &'a mut self,
            chars: &PickerChars,
            #[cfg(feature = "preview")] config: &PreviewConfig,
        ) -> Self::Preview<'a>;

        #[inline]
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
            W: Terminal,
        {
            let preview = self.make_preview(
                &core.chars,
                #[cfg(feature = "preview")]
                &core.preview_config,
            );
            core.pick_impl::<_, _, Self::Queued, _>(event_source, terminal, preview)
        }
    }
}

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
/// This trait is sealed and cannot be implemented outside this library.
///
/// Read the [module documentation](self) for an overview and more details.
#[allow(private_bounds)]
pub trait Interface<T>: sealed::State<T> {
    /// The output of the picker after selection. This is an [`Option<&T>`] for single-selection
    /// pickers and [`Selection<'_, T>`] for multi-selection pickers.
    type Output<'a>: IntoIterator<Item = &'a T, IntoIter: ExactSizeIterator>
    where
        T: 'a;

    /// The default keybindings for this interface.
    ///
    /// This method is generic over the abort type `A` for compatibility with arbitrary application
    /// errors. Interfaces use this to specialize event handling to only emit events which this
    /// specific interface requires. See the
    /// [keybind specialization documentation](crate::event::keybind_default#specialized-bindings)
    /// for more details.
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>>;
}

/// The single-selection picker interface.
///
/// Read the [module-level docs](self) or the [interface docs](Interface) for more details.
pub struct Single;

/// The multiple-selection picker interface.
///
/// Read the [module-level docs](self) or the [interface docs](Interface) for more details.
pub struct Multi;

/// The picker [interface](Interface) which displays item previews.
///
/// The type parameter `I` is the base interface, which in practice is either [`Single`] or
/// [`Multi`]. This interface owns the previewer itself, but do note that if `P` implements
/// [`Preview`], then so does `&mut P`, which can avoid destruction of stateful previewers when
/// relevant.
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
/// prefetch cache, separate from the main cache. The prefetch cache will be 2 times the size of [`Preview::prefetch_limit`].
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
pub struct WithPreview<I, P> {
    pub(crate) interface: I,
    pub(crate) previewer: P,
}

#[cfg(feature = "preview")]
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

#[cfg(feature = "preview")]
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

impl<T: Send + Sync + 'static> Interface<T> for Single {
    type Output<'a> = Option<&'a T>;

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_single(key)
    }
}

impl<T: Send + Sync + 'static> sealed::State<T> for Single {
    type Queued = ();
    type Preview<'a> = NoPreview;

    #[inline]
    fn make_preview<'a>(
        &'a mut self,
        _: &PickerChars,
        #[cfg(feature = "preview")] _: &PreviewConfig,
    ) -> Self::Preview<'a> {
        NoPreview
    }
}

impl<T: Send + Sync + 'static> Interface<T> for Multi {
    type Output<'a> = Selection<'a, T>;

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_multi(key)
    }
}

impl<T: Send + Sync + 'static> sealed::State<T> for Multi {
    type Queued = SelectedIndices;
    type Preview<'a> = NoPreview;

    #[inline]
    fn make_preview<'a>(
        &'a mut self,
        _: &PickerChars,
        #[cfg(feature = "preview")] _: &PreviewConfig,
    ) -> Self::Preview<'a> {
        NoPreview
    }
}

#[cfg(feature = "preview")]
impl<T: Send + Sync + 'static, P: Preview<T>> Interface<T> for WithPreview<Single, P> {
    type Output<'a> = Option<&'a T>;

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_single_preview(key)
    }
}

#[cfg(feature = "preview")]
impl<T: Send + Sync + 'static, P: Preview<T>> sealed::State<T> for WithPreview<Single, P> {
    type Queued = ();
    type Preview<'a>
        = PreviewPane<T, &'a mut P>
    where
        Self: 'a;

    #[inline]
    fn make_preview<'a>(
        &'a mut self,
        chars: &PickerChars,
        config: &PreviewConfig,
    ) -> Self::Preview<'a> {
        PreviewPane::new(config, chars, &mut self.previewer)
    }
}

#[cfg(feature = "preview")]
impl<T: Send + Sync + 'static, P: Preview<T>> Interface<T> for WithPreview<Multi, P> {
    type Output<'a> = Selection<'a, T>;

    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_multi_preview(key)
    }
}

#[cfg(feature = "preview")]
impl<T: Send + Sync + 'static, P: Preview<T>> sealed::State<T> for WithPreview<Multi, P> {
    type Queued = SelectedIndices;
    type Preview<'a>
        = PreviewPane<T, &'a mut P>
    where
        Self: 'a;

    #[inline]
    fn make_preview<'a>(
        &'a mut self,
        chars: &PickerChars,
        config: &PreviewConfig,
    ) -> Self::Preview<'a> {
        PreviewPane::new(config, chars, &mut self.previewer)
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
