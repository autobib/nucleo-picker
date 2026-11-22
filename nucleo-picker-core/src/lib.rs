//! # A generic fuzzy item picker
//!
//! This is a low-level crate for implementing a custom picker interfaces of a type similar to
//! [fzf](https://github.com/junegunn/fzf) directly in your own application.
//!
//! - For a high-level easy-to-use picker library, see [nucleo-picker](https://docs.rs/nucleo-picker) crate directly.
//! - For built-in interfaces, see [nucleo-picker-interface](https://docs.rs/nucleo-picker-interface).
//!
//! The main use-case of this crate is if you wish to fully customize the behaviour of the picker
//! interface in a way that is abstract over matching and rendering.
//!
//! ## Crate layout
//!
//! In short, a picker consists of three components:
//!
//! - The [`Model`](session::Model) contains the core resuable state and configuration for
//!   the picker engine. One might think af this as the minimal picker.
//! - The [`Interface`](interface::Interface) trait describes how the session interacts with the model, and
//!   handles things like preview generation and multiple selection.
//! - The interface and model are combined in a [`Session`](session::Session). A session is *a single
//!   interactive selection session*, i.e. the time from opening the interface to selecting an item.
//!
//! The [nucleo-picker-interface](https://docs.rs/nucleo-picker-interface) crate contains some standard [`Interface`](interface::Interface) implementations.
//! The [nucleo-picker](https://docs.rs/nucleo-picker) crate is a high-level picker runner.
//!
//! ## Cargo features
//!
//! This library uses Cargo features to enable and disable various behaviour, especially when that
//! behaviour would require additional dependencies to operate.
//! The features are as follows:
//!
//! | Feature | Default | Dependencies | Description |
//! | --- | --- | --- | --- |
//! | `serde` | No | [`serde`] | Adds a `DeserializeSeed` implementation for `&Injector`. |
//! | <code style="white-space: nowrap;">preview-ansi</code> | No | [`anstyle-parse`](anstyle_parse) | Adds ANSI-parsing support for the preview buffer. |
//! | <code style="white-space: nowrap;">ratatui</code> | No | [`ratatui-core`](ratatui_core) | Adds support for Ratatui styles and a convience widget API |
//! | `crossterm` | No | [`crossterm`] | Adds style conversions between Crossterm types and picker styles. |

#![deny(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod component;
pub mod draw;
pub mod event;
mod generation;
mod incremental;
mod injector;
pub mod interface;
mod match_engine;
mod match_list;
pub mod preview;
mod prompt;
mod rect;
pub mod render;
pub mod session;
mod status_line;
mod util;

use std::borrow::Cow;

use nucleo::pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization};

pub use generation::Generation;
pub use injector::Injector;

/// A trait which describes how to render objects for matching and display.
///
/// Some renderers for common types are already implemented in the [`render`] module. In
/// many cases, the [`DisplayRenderer`](render::DisplayRenderer) is particularly easy to use.
/// This trait is also automatically implemented for [closures which return `Cow<'a,
/// str>`](#impl-Render<T>-for-R).
///
/// Rendering *must* be **pure**: for a given render implementation `R` and an item `T`, the call
/// `R::render(&self, &T)` must depend only on the specific render instance and the specific item,
/// and not any other state. Violation of this condition is normally only possible via interior
/// mutability, global state, I/O, or unsafe code.
///
/// If purism is violated, internal index computations which depend on the rendered format
/// will become invalid and the picker may panic or return incorrect results. Note that such
/// errors are encapsulated within the picker and will not result in undefined behaviour.
///
/// ## Examples
/// Here is a basic example for how one would implement a renderer for a `DirEntry` from the
/// [ignore](https://docs.rs/ignore/latest/ignore/) crate.
/// ```
/// use std::borrow::Cow;
///
/// use nucleo_picker_core::Render;
/// use ignore::DirEntry;
///
/// pub struct DirEntryRenderer;
///
/// impl Render<DirEntry> for DirEntryRenderer {
///     type Str<'a> = Cow<'a, str>;
///
///     fn render<'a>(&self, item: &'a DirEntry) -> Self::Str<'a> {
///         item.path().to_string_lossy()
///     }
/// }
/// ```
/// Here is another example showing that a renderer can use internal (immutable) state to customize
/// the rendered format.
/// ```
/// use nucleo_picker_core::Render;
///
/// pub struct PrefixRenderer {
///     prefix: String,
/// }
///
/// impl<T: AsRef<str>> Render<T> for PrefixRenderer {
///     type Str<'a> = String
///         where T: 'a;
///
///     fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
///         let mut rendered = String::new();
///         rendered.push_str(&self.prefix);
///         rendered.push_str(item.as_ref());
///         rendered
///     }
/// }
/// ```
///
/// Here is an example using a function directly as a renderer.
///
/// ```
/// use std::borrow::Cow;
/// use nucleo_picker_core::session::Model;
///
/// struct Item {
///     name: String,
/// }
///
/// fn render(item: &Item) -> Cow<'_, str> {
///     Cow::Borrowed(&item.name)
/// }
///
/// let model = Model::new(render);
/// let item = Item { name: "README.md".into() };
/// assert_eq!(model.render(&item), "README.md");
/// model.push_batch([item]);
/// ```
///
/// ## Render considerations
/// The picker is capable of correctly displaying most Unicode data. Internally, Unicode width
/// calculations are performed to keep track of the amount of space that it takes on the screen to
/// display a given item.
///
/// The main exception is control characters which are not newlines (`\n` or `\r\n`). Even visible
/// control characters such as tabs (`\t`) will cause issues: width calculations will most likely
/// be incorrect since the amount of space a tab occupies depends on its position within the
/// screen.
///
/// It is best to avoid such characters in your rendered format. If you do not have control
/// over the incoming data, the most robust solution is likely to perform substitutions while
/// rendering.
/// ```
/// # use nucleo_picker_core::Render;
/// use std::borrow::Cow;
///
/// fn renderable(c: char) -> bool {
///     !c.is_control() || c == '\n'
/// }
///
/// struct ControlReplaceRenderer;
///
/// impl<T: AsRef<str>> Render<T> for ControlReplaceRenderer {
///     type Str<'a>
///         = Cow<'a, str>
///     where
///         T: 'a;
///
///     fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
///         let mut str = Cow::Borrowed(item.as_ref());
///
///         if str.contains(|c| !renderable(c)) {
///             str.to_mut().retain(renderable);
///         }
///
///         str
///     }
/// }
/// ```
///
/// ## Performance considerations
/// In the majority of situations, performance of the [`Render`] implementation is only relevant
/// when sending the items to the picker, and not for generating the match list interactively. In
/// particular, in the majority of situations, [`Render::render`] is called exactly once per item
/// when it is sent to the picker.
///
/// The **only** exception to this rule occurs when the value returned by [`Render::render`] contains
/// non-ASCII characters. In this situation, it can happen that *exceptionally slow* [`Render`]
/// implementations will reduce interactivity. A crude rule of thumb is that rendering a single
/// item should take (in the worst case) at most 100μs. For comparison, display formatting a
/// `f64` takes around 100ns.
///
/// 100μs is an extremely large amount of time in the vast majority of situations. If after
/// benchmarking you determine that this is not the case for your [`Render`] implementation,
/// and moreover your [`Render`] implementation outputs (in the majority of cases) non-ASCII
/// Unicode, you can internally cache the render computation (at the cost of increased memory
/// overhead):
/// ```
/// # use nucleo_picker_core::Render;
/// pub struct Item<D> {
///     data: D,
///     /// the pre-computed rendered version of `data`
///     rendered: String,
/// }
///
/// pub struct ItemRenderer;
///
/// impl<D> Render<Item<D>> for ItemRenderer {
///     type Str<'a>
///         = &'a str
///     where
///         D: 'a;
///
///     fn render<'a>(&self, item: &'a Item<D>) -> Self::Str<'a> {
///         &item.rendered
///     }
/// }
/// ```
pub trait Render<T> {
    /// The string type that `T` is rendered as, most commonly a [`&'a str`](str), a
    /// [`Cow<'a, str>`](std::borrow::Cow), or a [`String`].
    type Str<'a>: AsRef<str>
    where
        T: 'a;

    /// Render the given item as it should appear in the picker. See the
    /// [trait-level docs](Render) for more detail.
    fn render<'a>(&self, item: &'a T) -> Self::Str<'a>;
}

impl<T, R: for<'a> Fn(&'a T) -> Cow<'a, str>> Render<T> for R {
    type Str<'a>
        = Cow<'a, str>
    where
        T: 'a;

    #[inline]
    fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
        self(item)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
/// How to treat a case mismatch between two characters.
pub enum CaseMatching {
    /// Characters never match their case folded version (`a != A`).
    Respect,
    /// Characters always match their case folded version (`a == A`).
    Ignore,
    /// Act like [`Ignore`](CaseMatching::Ignore) if all characters in a pattern atom are
    /// lowercase and like [`Respect`](CaseMatching::Respect) otherwise.
    #[default]
    Smart,
}

impl CaseMatching {
    pub(crate) const fn convert(self) -> NucleoCaseMatching {
        match self {
            Self::Respect => NucleoCaseMatching::Respect,
            Self::Ignore => NucleoCaseMatching::Ignore,
            Self::Smart => NucleoCaseMatching::Smart,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
/// How to handle Unicode Latin normalization.
pub enum Normalization {
    /// Characters never match their normalized version (`a != ä`).
    Never,
    /// Act like [`Never`](Normalization::Never) if any character would need to be normalized, and
    /// otherwise perform normalization (`a == ä` but `ä != a`).
    #[default]
    Smart,
}

impl Normalization {
    pub(crate) const fn convert(self) -> NucleoNormalization {
        match self {
            Self::Never => NucleoNormalization::Never,
            Self::Smart => NucleoNormalization::Smart,
        }
    }
}

/// Decorative characters used by the picker UI.
///
/// All characters must occupy one terminal column.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub(crate) struct PickerChars {
    /// The prompt indicator.
    pub prompt: char,
    /// The marker for the hovered item.
    pub hover: char,
    /// The marker for any queued items.
    pub queued: char,
    /// An indicator for truncated text.
    pub ellipsis: char,
    /// The horizontal separator between the prompt and the match list.
    pub separator: char,
    /// The characters used by the item spinner.
    pub spinner_chars: &'static [char],
    /// An indicator that matching is ongoing in the background.
    pub matching_indicator: char,
}

impl PickerChars {
    /// The default Unicode character set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prompt: '>',
            hover: '▌',
            queued: '┃',
            ellipsis: '…',
            separator: '─',
            spinner_chars: &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            matching_indicator: '·',
        }
    }

    /// An ASCII-compatible character set.
    #[must_use]
    pub const fn ascii() -> Self {
        Self {
            prompt: '>',
            hover: '>',
            queued: '|',
            ellipsis: '.',
            separator: '-',
            spinner_chars: &['|', '|', '/', '/', '-', '-', '\\', '\\'],
            matching_indicator: '.',
        }
    }
}

impl Default for PickerChars {
    fn default() -> Self {
        Self::new()
    }
}
