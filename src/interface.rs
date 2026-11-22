//! # Interface generics
//!
//! This module implements interface generics. The interface is the type parameter `I` in
//! [`Picker<T, R, I>`][`Picker`] and describes the boundary between the core picker state and
//! externally managed state (for instance for previews and multiple selection).
//!
//! The main entrypoint is the [`Interface`] trait. However, you probably don't need to implement it
//! yourself; you can typically use one of the built-in interfaces defined in this module
//! (see [`Single`], [`Multi`], [`CachedPreview`], and [`UncachedPreview`]).
//!
//! ## Interfaces
//!
//! In order to choose an interface, start with [`Picker::new`](crate::Picker::new) for single-selection.
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
//! Note that [`CachedPreview`] requires the `preview-cache` feature. When the picker quits without a
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
//! [`Event::Select`](crate::event::Event::Select) is handled slightly differently: if there are no queued selections, this
//! picks the hovered item, but if there are queued selections, then only the queued selections
//! are returned.
//!
//! If the picker [restarts while running](crate::event::Event::Restart), the queued item list will be cleared
//! since the previous items are removed.
//!
//! The selected items are returned as a [`Selection`], which is empty if picker exited with
//! [`Event::Quit`](crate::event::Event::Quit) (or [`Event::QuitPromptEmpty`](crate::event::Event::QuitPromptEmpty)), and non-empty if not.
//!
//! ### Adding previews
//!
//! With the `preview-cache` feature enabled, [`Picker::with_preview`](crate::Picker::with_preview) wraps the current selection mode in
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
//! For an owned preview-enabled picker, [`Picker::into_parts`](crate::Picker::into_parts) recovers both the picker and
//! its previewer. [`Picker::without_preview`](crate::Picker::without_preview) discards the previewer instead.
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
//! Owned conversions such as [`Picker::multi`] and [`Picker::single`](crate::Picker::single) consume the picker and
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
//! a [`PickerRef`]. These methods are named like `as_*`, such as [`Picker::as_multi`] and [`Picker::as_single`](crate::Picker::as_single).
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
//! In order to explicitly choose an interface, use [`Picker::with_interface`](crate::Picker::with_interface) or [`Picker::as_interface`](crate::Picker::as_interface).
//!
//! ## Picker generics
//!
//! In most cases, the picker generic parameters can be inferred from the renderer, inserted items,
//! and the conversion methods directly. Regardless, here is an overview of the type parameters of a
//! [`Picker<T, R, I>`][`Picker`] and [`PickerRef<'a, T, R, I>`][`PickerRef`]:
//!
//! - `T` is the *item type*. Running the picker requires
//!   `T: Send + Sync + 'static`. Selected items are borrowed from the picker.
//! - `R` is the *renderer*, implementing [`Render<T>`](crate::Render).
//! - `I` is the *session interface*, defaulting to [`Single`]. Interactive methods require
//!   `I: Interface<T>`. Headless methods such as [`Picker::filter`](crate::Picker::filter) are independent of the
//!   interface.
//! - `'a` in `PickerRef<'a, T, R, I>` is the lifetime of the mutable borrow of the
//!   original picker. Its `pick*` methods return `I::Output<'a>`.
//!
//! In most cases you can use one of the provided interfaces. To customize how
//! items are displayed, implement [`Render`](crate::Render) or [`Preview`](crate::preview::Preview); to customize IO, use
//! [`Picker::pick_with_keybind`](crate::Picker::pick_with_keybind) or [`Picker::pick_with_io`](crate::Picker::pick_with_io).
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
//!
//! [`Picker`]: crate::Picker
//! [`Picker::multi`]: crate::Picker::multi
//! [`Picker::as_multi`]: crate::Picker::as_multi
//! [`PickerRef`]: crate::PickerRef

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub use nucleo_picker_interface::preview;
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
#[doc(inline)]
pub use nucleo_picker_interface::{CachedPreview, CachedPreviewState};
#[doc(inline)]
pub use nucleo_picker_interface::{
    Interface, Multi, MultiState, Selection, SelectionIntoIter, Single, UncachedPreview,
    UncachedPreviewState,
};
