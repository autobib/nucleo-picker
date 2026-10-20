//! # A generic fuzzy item picker
//!
//! This crate contains a generic picker implementation that allows you to natively incorporate an
//! interactive fuzzy picker TUI (similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf)) directly in your own applications.
//!
//! In short, initialize a [`Picker`] using [`PickerOptions`] and describe how the items
//! should be represented by implementing [`Render`], or use a [built-in renderer](render).
//!
//! To include a preview pane in the picker, enable the [preview feature](#cargo-features) and see the [`preview`]
//! module.
//! feature-aware documentation or an explicit documentation URL.]
//! For more complex use-cases and integration with an existing application, see the
//! [`event`] module.
//!
//! ## Usage examples
//! For many usage examples, visit the [examples
//! folder](https://github.com/autobib/nucleo-picker/tree/master/examples) on GitHub.
//!
//! ### `fzf` example
//! Run this example with `cat myfile.txt | cargo run --release --example fzf_basic`.
//! ```no_run
#![doc = include_str!("../examples/fzf_basic.rs")]
//! ```
//!
//! ### `find` example
//! Run this example with `cargo run --release --example find ~`.
//! ```no_run
#![doc = include_str!("../examples/find.rs")]
//! ```
//!
//! ## Cargo features
//!
//! This library uses Cargo features to enable and disable various behaviour, especially when that
//! behaviour would require additional dependencies to operate.
//! The features are as follows:
//!
//! | Feature | Default | Dependencies | Requires | Description |
//! | --- | --- | --- | --- | --- |
//! | <code style="white-space: nowrap;">unix-poll</code> | Yes | — | — | This enables the <code style="white-space: nowrap;">use-dev-tty</code> feature of `crossterm` to fix a polling issue on macOS which breaks non-interactive STDIN. (Note that the <code style="white-space: nowrap;">use-dev-tty</code> feature is confusingly named, since even with this feature disabled, crossterm uses `/dev/tty` anyway.) |
//! | `preview` | No | [`lru`] | — | This enables the base preview-pane functionality. |
//! | `serde`   | No | [`serde`] | — | This enables `DeserializeSeed` for an [`Injector`]. |
//! | <code style="white-space: nowrap;">preview-ansi</code> | No | [`anstyle_parse`] | `preview` | This adds ANSI-parsing support for the preview buffer. |

#![deny(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod component;
mod core;
pub mod error;
pub mod event;
mod frame;
mod headless;
mod incremental;
mod injector;
mod interface;
mod match_engine;
mod match_list;
mod observer;
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
pub mod preview;
mod prompt;
mod rect;
pub mod render;
mod session;
mod status_line;
mod terminal;
mod util;

use std::{borrow::Cow, io, iter::Extend, num::NonZero, time::Duration};

use crossterm::event::KeyEvent;
use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};
use observer::Observer;
#[cfg(feature = "preview")]
use preview::{BoundaryChars, PreviewConfig};

use crate::{
    core::PickerCore,
    error::PickError,
    event::{Event, EventSource, PickerStatus},
    match_engine::MatchEngine,
    match_list::{MatchListConfig, MatchListState},
    prompt::{PromptConfig, PromptState},
};

pub use crate::headless::{MatchStatus, Matches};
pub use crate::injector::Injector;
pub use crate::interface::{Interface, Multi, PickerRef, Single};
pub use crate::match_list::{Selection, SelectionIntoIter};
#[cfg(feature = "preview")]
pub use crate::preview::WithPreview;

#[cfg(feature = "unstable-backend")]
pub use terminal::Terminal;
#[cfg(not(feature = "unstable-backend"))]
pub(crate) use terminal::Terminal;

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
/// use nucleo_picker::Render;
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
/// use nucleo_picker::Render;
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
/// use nucleo_picker::Picker;
///
/// struct Item {
///     name: String,
/// }
///
/// fn render(item: &Item) -> Cow<'_, str> {
///     Cow::Borrowed(&item.name)
/// }
///
/// let picker = Picker::new(render);
/// let item = Item { name: "README.md".into() };
/// assert_eq!(picker.render(&item), "README.md");
/// picker.push_batch([item]);
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
/// # use nucleo_picker::Render;
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
/// # use nucleo_picker::Render;
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
/// Note that all of the supplied characters must be single-width, or the picker screen will be
/// corrupted.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub(crate) struct PickerChars {
    /// The prompt indicator.
    pub prompt: char,
    /// The marker for the currently selected item.
    pub selection: char,
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
            selection: '▌',
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
            selection: '>',
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

/// Specify configuration options for a [`Picker`].
///
/// Initialize with [`new`](PickerOptions::new) or (equivalently) the
/// [`Default`](PickerOptions::default) implementation, specify options, and then convert to a
/// [`Picker`] using the [`picker`](PickerOptions::picker) method.
///
/// ## Example
/// ```
/// use nucleo_picker::{render::StrRenderer, Picker, PickerOptions};
///
/// let picker: Picker<String, _> = PickerOptions::new()
///     .highlight(true)
///     .query("search")
///     .picker(StrRenderer);
/// ```
///
/// ## Sort order settings
///
/// There are three settings which influence the order in which items appear on the screen.
///
/// The [`reversed`](Self::reversed) setting only influences the layout: by default, the query is
/// placed at the bottom of the screen, with the match list rendered bottom-up. If this is set, the query will be placed at the top of the screen, with the match list rendered top-down.
///
/// The [`reverse_items`](Self::reverse_items) and [`sort_results`](Self::sort_results) are used to
/// determine *the order of the items inside the match list*. The order is defined according to the
/// following table. Here, `Index` refers to the order in which the `picker` received the items
/// from the [`Injector`]s. If you use exactly one injector, this is guaranteed to be the same as
/// the insertion order.
///
/// | `sort_results` | `reverse_items` | Sort priority                              |
/// |----------------|-----------------|--------------------------------------------|
/// | `true`         | `false`         | Score (desc) → Length (asc) → Index (asc)  |
/// | `true`         | `true`          | Score (desc) → Length (asc) → Index (desc) |
/// | `false`        | `false`         | Index (asc)                                |
/// | `false`        | `true`          | Index (desc)                               |
pub struct PickerOptions {
    config: nc::Config,
    query: String,
    threads: Option<NonZero<usize>>,
    max_selection_count: Option<NonZero<u32>>,
    interval: Duration,
    background_frame_interval: Duration,
    chars: PickerChars,
    match_list_config: MatchListConfig,
    case_matching: NucleoCaseMatching,
    normalization: NucleoNormalization,
    prompt_config: PromptConfig,
    #[cfg(feature = "preview")]
    preview_config: PreviewConfig,
    sort_results: bool,
    reverse_items: bool,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    ///
    /// Equivalent to the [`Default`] implementation, but as a `const fn`.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            config: nc::Config::DEFAULT,
            query: String::new(),
            threads: None,
            max_selection_count: None,
            interval: Duration::from_millis(15),
            background_frame_interval: Duration::from_millis(60),
            chars: PickerChars::new(),
            match_list_config: MatchListConfig::new(),
            case_matching: NucleoCaseMatching::Smart,
            normalization: NucleoNormalization::Smart,
            prompt_config: PromptConfig::new(),
            #[cfg(feature = "preview")]
            preview_config: PreviewConfig::new(),
            sort_results: true,
            reverse_items: false,
        }
    }

    /// Convert into a [`Picker`].
    #[must_use]
    pub fn picker<T: Send + Sync + 'static, R>(self, render: R) -> Picker<T, R> {
        let background_frame_frequency = {
            let frame_interval = self.interval;
            let background_frame_interval = self.background_frame_interval;
            let frame_nanos = frame_interval.as_nanos();
            let frames = if frame_nanos == 0 {
                1
            } else {
                background_frame_interval
                    .as_nanos()
                    .div_ceil(frame_nanos)
                    .max(1)
            };
            NonZero::new(usize::try_from(frames).unwrap_or(usize::MAX)).unwrap()
        };

        let reversed = self.match_list_config.reversed;

        let list_state = MatchListState::new(self.match_list_config, self.config.clone());
        let mut engine = MatchEngine::new(
            self.config,
            nc::MatchListConfig {
                sort_results: self.sort_results,
                reverse_items: self.reverse_items,
            },
            self.threads,
            render.into(),
            self.case_matching,
            self.normalization,
        );

        let mut prompt = PromptState::new(self.prompt_config);

        // set the prompt
        prompt.set_query(self.query);
        engine.reparse(prompt.contents());

        PickerCore {
            engine,
            list_state,
            prompt,
            #[cfg(feature = "preview")]
            preview_config: self.preview_config,
            interval: self.interval,
            background_frame_frequency,
            chars: self.chars,
            max_selection_count: self.max_selection_count,
            reversed,
            restart_notifier: None,
            status_notifier: None,
        }
        .with_interface(Single)
    }

    /// Set a 'reversed' rendering order in the interface.
    ///
    /// Option `false` (default) will put the prompt at the bottom and render items in from the
    /// bottom to the top. Option `true` will put the prompt at the top and render items from the
    /// top to the bottom.
    ///
    /// This does not influence the sort order of the items themselves.
    #[must_use]
    #[inline]
    pub const fn reversed(mut self, reversed: bool) -> Self {
        self.match_list_config.reversed = reversed;
        self
    }

    /// Reverse the item insert order.
    ///
    /// This changes the index tie-break method to prefer later indices rather than earlier
    /// indices. This option is typically used with [`sort_results`](Self::sort_results) set
    /// to `false`, in which case the newest items sent to the picker will be placed
    /// first, rather than last.
    ///
    /// The default is `false`.
    #[must_use]
    #[inline]
    pub const fn reverse_items(mut self, reversed: bool) -> Self {
        self.reverse_items = reversed;
        self
    }

    /// Whether or not to sort matching items by score.
    ///
    /// This option is useful when you just want to filter items, but preserve the original order.
    /// When this option is set to `false`, the oldest items will appear at the beginning and the newest items will appear at the end.
    /// This can be swapped by setting [`reverse_items`](Self::reverse_items) to `true`.
    ///
    /// The default value is `true`.
    #[must_use]
    #[inline]
    pub const fn sort_results(mut self, sort: bool) -> Self {
        self.sort_results = sort;
        self
    }

    /// The maximum number of items that can be selected in the picker.
    ///
    /// If `None`, no bound is applied; otherwise, use the provided bound. This is a `u32` since
    /// the picker cannot hold more than [`u32::MAX`] items.
    ///
    /// This bound is only relevant when allowing [multiple selections](Picker#multiple-selections)
    /// and has no impact on single selection mode. The returned [`Selection`] will contain at
    /// most `maximum` items. The picker will not allow the selection of additional items if the maximum
    /// is reached.
    ///
    /// The default is `None`.
    #[must_use]
    #[inline]
    pub const fn max_selection_count(mut self, maximum: Option<NonZero<u32>>) -> Self {
        self.max_selection_count = maximum;
        self
    }

    /// Set how long each frame should last.
    ///
    /// This is the reciprocal of the refresh rate. The default value is
    /// `Duration::from_millis(15)`, which corresponds to a refresh rate of approximately 67 frames
    /// per second. It is not recommended to set this to a value less than 8ms (approximately 125
    /// frames per second).
    #[must_use]
    #[inline]
    pub const fn frame_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Set the number of threads used by the internal matching engine.
    ///
    /// If `None` (default), use a heuristic choice based on the amount of available
    /// parallelism along with other factors.
    #[must_use]
    #[inline]
    pub const fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.threads = threads;
        self
    }

    /// How to perform Unicode normalization (defaults to [`Normalization::Smart`]).
    #[must_use]
    #[inline]
    pub const fn normalization(mut self, normalization: Normalization) -> Self {
        self.normalization = normalization.convert();
        self
    }

    /// How to treat case mismatch (defaults to [`CaseMatching::Smart`]).
    #[must_use]
    #[inline]
    pub const fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.case_matching = case_matching.convert();
        self
    }

    /// Enable score bonuses appropriate for matching file paths.
    #[must_use]
    #[inline]
    pub const fn match_paths(mut self) -> Self {
        self.config = self.config.match_paths();
        self
    }

    /// Whether to provide a bonus to matches by their distance from the start of the item.
    ///
    /// This is disabled by default and only recommended for autocompletion use-cases, where the expectation is that the user is typing the entire match.
    #[must_use]
    #[inline]
    pub const fn prefer_prefix(mut self, prefer_prefix: bool) -> Self {
        self.config.prefer_prefix = prefer_prefix;
        self
    }

    /// Whether or not to highlight matches (defaults to `true`).
    #[must_use]
    #[inline]
    pub const fn highlight(mut self, highlight: bool) -> Self {
        self.match_list_config.highlight = highlight;
        self
    }

    /// Whether to highlight the entire line, and not only the match text
    ///
    /// This can be useful for distinguishing multiline items from each other. The default is false.
    #[must_use]
    #[inline]
    pub const fn highlight_line(mut self, highlight_line: bool) -> Self {
        self.match_list_config.highlight_line = highlight_line;
        self
    }

    /// How much space to leave when rendering match highlighting (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn highlight_padding(mut self, size: u16) -> Self {
        self.match_list_config.highlight_padding = size;
        self
    }

    /// How much space to leave around the selection when scrolling (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn scroll_padding(mut self, size: u16) -> Self {
        self.match_list_config.scroll_padding = size;
        self
    }

    /// How much space to leave around the cursor (defaults to `2`).
    #[must_use]
    #[inline]
    pub const fn prompt_padding(mut self, size: u16) -> Self {
        self.prompt_config.padding = size;
        self
    }

    /// Set the character used for the prompt indicator.
    ///
    /// The default value is `'>'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn prompt_indicator(mut self, prompt: char) -> Self {
        self.chars.prompt = prompt;
        self
    }

    /// Set the character used to mark the currently selected item.
    ///
    /// The default value is `'▌'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn selection_indicator(mut self, selection: char) -> Self {
        self.chars.selection = selection;
        self
    }

    /// Set the character used to indicate items queued for selection
    ///
    /// The default value is `'┃'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn queued_indicator(mut self, queued: char) -> Self {
        self.chars.queued = queued;
        self
    }

    /// Set the character used to indicate text that has been truncated.
    ///
    /// The default value is `'…'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn truncation_ellipsis(mut self, ellipsis: char) -> Self {
        self.chars.ellipsis = ellipsis;
        self
    }

    /// Set the character used for the horizontal rule separating the prompt and the match list.
    ///
    /// The default value is `'─'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn separator(mut self, sep: char) -> Self {
        self.chars.separator = sep;
        self
    }

    /// Set the characters used to animate the input status spinner.
    ///
    /// While the picker may still receive items, an indicator cycles through the characters.
    /// An empty slice disables the indicator entirely. The default value is
    /// `&['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']`.  The characters
    /// must all have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn spinner_chars(mut self, spinner: &'static [char]) -> Self {
        self.chars.spinner_chars = spinner;
        self
    }

    /// Set the character used to indicate that the matcher is working.
    ///
    /// This indicator is only displayed when the picker is no longer waiting for items; otherwise,
    /// the active spinner takes priority. The default value is `'·'`. The character must
    /// have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn matching_indicator(mut self, indicator: char) -> Self {
        self.chars.matching_indicator = indicator;
        self
    }

    /// Override all previously set UI characters to a default set.
    ///
    /// If `ascii` is `true` this uses an ASCII-compatible set.
    /// This is equivalent to:
    /// ```
    /// # use nucleo_picker::PickerOptions;
    /// PickerOptions::new()
    ///     .prompt_indicator('>')
    ///     .selection_indicator('>')
    ///     .queued_indicator('|')
    ///     .truncation_ellipsis('.')
    ///     .separator('-')
    ///     .spinner_chars(&['|', '|', '/', '/', '-', '-', '\\', '\\'])
    ///     .matching_indicator('.');
    /// ```
    /// If `ascii` is `false`, this uses the default Unicode character set.
    ///
    /// When the `preview` feature is enabled, this also sets the preview boundary characters to use
    /// the ASCII or Unicode defaults.
    pub const fn ascii_compatible(mut self, ascii: bool) -> Self {
        self.chars = if ascii {
            PickerChars::ascii()
        } else {
            PickerChars::new()
        };
        #[cfg(feature = "preview")]
        {
            self.preview_config.boundary_chars = if ascii {
                BoundaryChars::ascii()
            } else {
                BoundaryChars::new()
            };
        }
        self
    }

    /// Set the interval between background frames.
    ///
    /// This is the frame interval used for items on the screen which are only occasionally updated.
    /// Currently, this is used for:
    ///
    /// - the input status spinner
    /// - the matcher status marker
    ///
    /// The interval is converted to a whole number of regular frames, rounding up. The default
    /// value is 60ms. You can tune the spinner appearance,
    /// independently of this timer, by repeating characters in
    /// [`spinner_chars`](Self::spinner_chars).
    #[must_use]
    #[inline]
    pub const fn background_frame_interval(mut self, interval: Duration) -> Self {
        self.background_frame_interval = interval;
        self
    }

    /// Provide an initial query string for the prompt (defaults to `""`).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.query = query.into();
        self
    }
}

#[cfg(feature = "preview")]
impl PickerOptions {
    /// Set the preview size as a fraction of the total terminal width.
    ///
    /// This value must be finite and in the range `0..=1`.
    ///
    /// # Panics
    ///
    /// Values outside the range `0..=1` will cause a panic in debug builds, but in release builds
    /// the value will simply be set to `0.5`
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[must_use]
    #[inline]
    pub const fn preview_size(mut self, ratio: f64) -> Self {
        let valid = ratio >= 0.0 && ratio <= 1.0;
        debug_assert!(valid, "preview ratio must be in 0..=1");
        self.preview_config.ratio = if valid { ratio } else { 0.5 };
        self
    }

    /// Set the preview boundary characters.
    ///
    /// This defaults to the value returned by [`BoundaryChars::new`]. Note that each character must
    /// have Unicode width 1.
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[must_use]
    #[inline]
    pub const fn preview_boundary_chars(mut self, chars: BoundaryChars) -> Self {
        self.preview_config.boundary_chars = chars;
        self
    }

    /// Set the capacity of the preview cache, or `None` for an unbounded cache.
    ///
    /// The preview cache is used to reduce preview requests when scrolling onto an item and to
    /// cache preview pane state (scroll, line numbers, etc.) The default cache size is 128.
    ///
    /// Use `None` with caution: this will result in new buffers being allocated for *every
    /// preview pane*. This option is mainly intended for exceptionally slow previews with a fixed
    /// bound on the number of items.
    ///
    /// # Cache size 1
    ///
    /// For fast (synchronous-only) previewers, setting a cache size of 1 can be useful to reduce
    /// allocations. Note however that the cache is also used to store scroll state, so a cache size
    /// of 1 will lose the scroll position every time the selection changes.
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[must_use]
    #[inline]
    pub const fn preview_cache_size(mut self, size: Option<NonZero<usize>>) -> Self {
        self.preview_config.cache_size = size;
        self
    }
}

/// A fuzzy matching interactive item picker.
///
/// The parameter `T` is the item type and the parameter `R` is the [renderer](Render), which
/// describes how to represent `T` in the match list.
///
/// Initialize a picker with [`Picker::new`], or with custom configuration using
/// [`PickerOptions`], and add elements to the picker using an [`Injector`] returned
/// by the [`Picker::injector`] method.
/// ```
/// use nucleo_picker::{render::StrRenderer, Picker};
///
/// // Initialize a picker using default settings, with item type `String`
/// let picker: Picker<String, _> = Picker::new(StrRenderer);
/// ```
///
/// See also the [usage
/// examples](https://github.com/autobib/nucleo-picker/tree/master/examples).
///
/// ## Picker variants
///
/// The default picker can be run in a number of different modes:
///
/// 1. The simplest (and most common) method is to use [`Picker::pick`].
/// 2. If you wish to customize keybindings, use [`Picker::pick_with_keybind`].
/// 3. If you wish to customize all IO to the picker, use [`Picker::pick_with_io`].
///
/// On success, these methods return `Option<&T>` as the return type, where `None` indicates that
/// no items were selected.
///
/// More generally, the picker uses a type-state pattern with a parameter `I` to define the
/// behaviour of the above 3 methods. Currently, the parameter `I` is used to define the
/// following behaviour:
///
/// 1. Whether the picker operates in single selection mode (the default [`Single`]) or in multiple
///    selection mode ([`Multi`]).
/// 2. Whether the picker has item previews (the wrapper state [`WithPreview`]).
///
/// ### Multiple selections
///
/// With the [`Multi`] or [`WithPreview<Multi, P>`] interface, the interactive picker permits the
/// user to make multiple selections.
///
/// The `pick*` methods are analogous to the single-selection variants, except additional items can be
/// queued with the [`MatchListEvent::ToggleDown`](crate::event::MatchListEvent::ToggleDown) and
/// [`MatchListEvent::ToggleUp`](crate::event::MatchListEvent::ToggleUp) events. The [default
/// keybindings](event::keybind_default) bind these to `⇥` and `shift + ⇥` respectively. In this case, an
/// [`Event::Select`] is handled slightly differently: if there are no queued selections, this
/// picks the highlighted item, but if there are queued selections, then only the queued selections
/// are returned.
///
/// If the picker [restarts while running](Event::Restart), the queued item list will be cleared
/// since the previous items are removed.
///
/// The selected items are returned as a [`Selection`], which is empty if picker exited with
/// [`Event::Quit`] (or [`Event::QuitPromptEmpty`]), and non-empty if not.
///
/// You can also use [`Picker::as_multi`] to temporarily mutably borrow as a multiple-selection
/// picker.
/// For instance, this enables patterns like `picker.as_multi().pick()` without owned type
/// conversion.
///
/// ### Previewer
///
/// In order to use previews, enable the `preview` feature and pass a previewer to the
/// picker with [`Picker::with_preview`]. The corresponding pick methods on the resulting
/// [`Picker`] have same behaviour, except with an additional preview pane rendered on the
/// right hand side of the screen. There is also additional handling of events which target the
/// preview pane in particular.
///
/// ### Headless (non-interactive) use
///
/// There is also basic support for non-interactive use.
/// This can be useful when you want to drive the picker without actually opening or rendering to a
/// terminal. Usage does not depend on the choice of interface (because the picker is
/// non-interactive).
///
/// The main entry points are:
///
/// - [`Picker::filter`]: wait for matching to finish, and return the matches for the current query.
/// - [`Picker::update_matches`]: advance matching and see the current status.
/// - [`Picker::matches`]: view the current matches.
///
/// ### Mutably borrow with a different interface
///
/// It is possible to mutably borrow a [`Picker`] with a different interface. This is
/// done using a special [`PickerRef`] type which mutably borrows the original picker but has a
/// different interface parameter.
///
/// ```no_run
/// use nucleo_picker::{Picker, error::PickError, render::StrRenderer};
///
/// # fn main() -> Result<(), PickError> {
/// let mut picker = Picker::new(StrRenderer);
/// picker.push_batch(["red", "blue", "green"]);
///
/// // borrow as a multi-selection picker without consuming
/// // the original picker. the selection lifetime is independent
/// // of the view type
/// for color in picker.as_multi().pick()? {
///     println!("{color}");
/// }
///
/// // the original picker is still a single-selection picker
/// if let Some(color) = picker.pick()? {
///     println!("{color}");
/// }
/// # Ok(())
/// # }
/// ```
///
/// See the [`PickerRef`] documentation for more details.
///
/// ### Generics
///
/// The [`Picker`] has a number of methods which are shared by all interfaces. For the interactive
/// `pick*` methods which require an interface, the interface type parameter must satisfy the [`Interface`] trait.
/// This makes it easier to write code which is generic over all picker types:
///
/// ```no_run
/// use nucleo_picker::{Interface, Picker, Render, error::PickError, render::StrRenderer};
///
/// // print the selected items, independent of the picker type
/// fn print_selection<T, R, I>(picker: &mut Picker<T, R, I>) -> Result<(), PickError>
/// where
///     T: Send + Sync + 'static + std::fmt::Display,
///     R: Render<T>,
///     I: Interface<T>,
/// {
///     for item in picker.pick()? {
///         println!("{item}");
///     }
///     Ok(())
/// }
///
/// # fn main() -> Result<(), PickError> {
/// let mut picker = Picker::new(StrRenderer);
/// picker.push_batch(["red", "blue", "green"]);
///
/// print_selection(&mut picker)?;
/// print_selection(&mut picker.multi())?;
/// # Ok(())
/// # }
/// ```
///
/// ### Emulate single selection using a multi-picker
///
/// It is possible to emulate single selection with multi-picker methods by setting
/// [`PickerOptions::max_selection_count`] to `NonZero::new(1)`. This will force the resulting
/// [`Selection`] to contain either 0 or 1 element, and you can convert to `Option<&T>` by calling
/// `next` on the [iterator](Selection::iter).
///
/// Note that the picker interface will be slightly different: it is still possible to
/// queue at most one picked item using `⇥`. With the single-selection pickers, it is not
/// possible to queue items at all.
///
/// ## Reusing the picker
///
/// By default, the picker **maintains state between sessions**. For example, calling [`Picker::pick`],
/// processing a selection, and then calling it again, will retain the prompt and items
/// from the previous sessions. This can be useful when resuming from errors (in
/// particular, for handling application-defined errors or IO errors).
///
/// To reuse the picker with new items and cleared prompt use [`Picker::restart`]. If you want to
/// retain the items and continue use of existing injectors, reset only the prompt by calling
/// [`Picker::reset_query`].
///
/// ```no_run
/// use nucleo_picker::{Picker, render::StrRenderer};
///
/// # fn main() -> std::io::Result<()> {
/// let mut picker = Picker::new(StrRenderer);
/// picker.injector().push_batch(["red", "blue"]);
/// if let Some(color) = picker.pick()? {
///     println!("{color}");
/// }
///
/// picker.restart();
/// picker.injector().push_batch(["small", "large"]);
/// if let Some(size) = picker.pick()? {
///     println!("{size}");
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## A note on memory usage
/// Initializing a picker is a relatively expensive operation since the internal match engine uses
/// an arena-based memory approach to minimize allocator costs, and this memory is initialized when
/// the picker is created.
///
/// To re-use the picker with reduced start-up costs, use [`Picker::restart`]. This allocates space
/// for the new items but reuses matching infrastructure.
///
/// # Example
///
/// Run the picker on [`Stdout`](std::io::Stdout) with no interactivity checks, and quitting
/// gracefully on `ctrl + c`.
/// ```no_run
#[doc = include_str!("../examples/custom_io.rs")]
/// ```
pub struct Picker<T, R, I = Single> {
    core: PickerCore<T, R>,
    interface: I,
}

impl<T: Send + Sync + 'static, R: Render<T>, I> Extend<T> for Picker<T, R, I> {
    #[inline]
    fn extend<U: IntoIterator<Item = T>>(&mut self, iter: U) {
        self.push_batch(iter);
    }
}

impl<T: Send + Sync + 'static, R> Picker<T, R> {
    /// Initialize a new picker with default configuration and the provided renderer.
    #[must_use]
    #[inline]
    pub fn new(render: R) -> Self
    where
        R: Render<T>,
    {
        PickerOptions::default().picker(render)
    }
}

/// Inherent picker methods which are independent of the interface.
impl<T: Send + Sync + 'static, R, I> Picker<T, R, I> {
    /// Open the interactive picker prompt and return the picked item, if any.
    ///
    /// ## Interface
    ///
    /// The behaviour of this function is influenced by the [`Interface`] parameter (for instance,
    /// returning a single or multiple items; or rendering a preview). Read the [interface
    /// docs](Interface) for more details.
    ///
    /// ## Stderr lock
    /// The picker prompt is rendered in an alternate screen using the `stderr` file handle. In
    /// order to prevent screen corruption, a lock is acquired to `stderr`; see
    /// [`StderrLock`](std::io::StderrLock) for more detail.
    ///
    /// In particular, while the picker is interactive, any other thread which attempts to write to
    /// stderr will block. Note that `stdin` and `stdout` will remain fully interactive.
    ///
    /// ## IO customization
    ///
    /// To further customize the IO behaviour of the picker, such as to provide your own writer
    /// (for instance to write to [`Stdout`](std::io::Stdout) instead) or use custom keybindings,
    /// see the [`pick_with_io`](Self::pick_with_io)  and
    /// [`pick_with_keybind`](Self::pick_with_keybind) methods.
    ///
    /// # Errors
    /// Underlying IO errors from the standard library or [`crossterm`] will be propagated with the
    /// [`PickError::IO`] variant.
    ///
    /// This method also fails with:
    ///
    /// 1. [`PickError::NotInteractive`] if stderr is not interactive.
    /// 2. [`PickError::UserInterrupted`] if the user presses `ctrl + c`.
    ///
    /// This method will **never** return [`PickError::Disconnected`].
    #[inline]
    pub fn pick(&mut self) -> Result<I::Output<'_>, PickError>
    where
        R: Render<T>,
        I: Interface<T>,
    {
        self.pick_with_keybind(I::default_keybind)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// The picker prompt is rendered in an alternate screen using the `stderr` file handle. See
    /// the [`pick`](Self::pick) method for more detail.
    ///
    /// To further customize event generation, see the [`pick_with_io`](Self::pick_with_io) method.
    /// The [`pick`](Self::pick) method is internally a call to this method with keybindings
    /// provided by [`keybind_default`](event::keybind_default).
    ///
    /// ## Interface
    ///
    /// The behaviour of this function is influenced by the [`Interface`] parameter (for instance,
    /// returning a single or multiple items; or rendering a preview). Read the [interface
    /// docs](Interface) for more details.
    ///
    /// # Errors
    ///
    /// Underlying IO errors from the standard library or [`crossterm`] will be propagated with the
    /// [`PickError::IO`] variant.
    ///
    /// This method also fails with:
    ///
    /// 1. [`PickError::NotInteractive`] if stderr is not interactive.
    /// 2. [`PickError::UserInterrupted`] if a keybinding results in a [`Event::UserInterrupt`],
    /// 3. [`PickError::Disconnected`] if an [`Event::Restart`] is processed and there is no
    ///    [injector observer](Self::injector_observer) available.
    #[inline]
    pub fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<I::Output<'_>, PickError>
    where
        R: Render<T>,
        I: Interface<T>,
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        interface::pick_with_keybind(&mut self.core, &mut self.interface, keybind)
    }

    /// Run the picker interactively with a custom event source and writer, returning the selected
    /// item, if any.
    ///
    /// The picker is rendered using the given writer. In most situations, you want to check that
    /// the writer is interactive using, for instance, [`IsTerminal`](io::IsTerminal). The picker reads
    /// events from the [`EventSource`] to update the screen. See the docs for [`EventSource`]
    /// for more detail.
    ///
    /// # Errors
    /// Underlying IO errors from the standard library or [`crossterm`] will be propagated with the
    /// [`PickError::IO`] variant.
    ///
    /// Whether or not this fails with another [`PickError`] variant depends on the [`EventSource`]
    /// implementation:
    ///
    /// 1. If [`EventSource::recv_timeout`] fails with a [`RecvError::Disconnected`](event::RecvError::Disconnected), the error
    ///    returned will be [`PickError::Disconnected`].
    /// 2. The error will be [`PickError::UserInterrupted`] if the [`Picker`] receives an
    ///    [`Event::UserInterrupt`].
    /// 3. The error will be [`PickError::Aborted`] if the [`Picker`] receives an
    ///    [`Event::Abort`].
    ///
    /// This method will **never** return [`PickError::NotInteractive`] since interactivity checks
    /// are not done.
    #[inline]
    pub fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<I::Output<'_>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        I: Interface<T>,
        E: EventSource,
        W: io::Write,
    {
        interface::pick_with_io(&mut self.core, &mut self.interface, event_source, writer)
    }

    /// Returns the contents of the query string internal to the picker.
    ///
    /// If called after running [`Picker::pick`], this will contain the contents of the query string
    /// at the moment that the item was selected or the picker quit.
    #[must_use]
    #[inline]
    pub fn query(&self) -> &str {
        self.core.prompt.contents()
    }

    /// Update the default query string. This is mainly useful for modifying the query string
    /// before re-using the [`Picker`].
    ///
    /// See the [`PickerOptions::query`] method to set the query during initialization, and
    /// [`PromptEvent::Reset`](event::PromptEvent::Reset) to reset the query during interactive
    /// use.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.core.prompt.set_query(query);
        self.core.engine.reparse(self.core.prompt.contents());
    }

    /// Reset the query string to the empty string. This is a convenience method to call
    /// [`update_query`](Self::update_query) with query string `""`.
    #[inline]
    pub fn reset_query(&mut self) {
        self.update_query(String::new());
    }

    /// Returns an [`Observer`] containing up-to-date [`Injector`]s for this picker.
    ///
    /// This is the channel to which new injectors will be sent when the picker processes a
    /// [restart event](Event::Restart). Restart events are not generated by this library. You only
    /// need this channel if you generate restart events in your own code. See the [`Event`] documentation
    /// for more detail.
    ///
    /// Calling this method will *invalidate all earlier injector observers*. If you want multiple copies of
    /// the same observer, clone your existing observer.
    ///
    /// If `with_injector` is `true`, the channel is initialized with an injector currently valid
    /// for the picker on creation.
    #[must_use]
    #[inline]
    pub fn injector_observer(&mut self, with_injector: bool) -> Observer<Injector<T, R>> {
        let (notifier, observer) = if with_injector {
            observer::occupied_channel(self.injector())
        } else {
            observer::channel()
        };
        self.core.restart_notifier = Some(notifier);
        observer
    }

    /// Returns an [`Observer`] containing up-to-date responses from [status
    /// requests](event::Event::Status).
    ///
    /// This is the channel to which status updates are sent when the picker processes a [status
    /// request](Event::Status). Status requests are not generated by this library. You only need this
    /// channel if you generate status requests in your own code. Read the [status event docs](event::Event#status)
    /// for more detail.
    ///
    /// Calling this method will *invalidate all earlier status observers*. If you want multiple
    /// copies of the same observer, clone your existing observer.
    #[must_use]
    #[inline]
    pub fn status_observer(&mut self) -> Observer<PickerStatus> {
        let (notifier, observer) = observer::channel();
        self.core.status_notifier = Some(notifier);
        observer
    }

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// All items are removed immediately. Existing injectors will continue to function but the
    /// items will no longer be received by this instance. The old items will only be dropped when
    /// all injectors are dropped.
    ///
    /// When previews are enabled, the old items will also only drop when all deferred preview
    /// requests (perhaps owned by the previewer) are dropped as well. See the [preview request
    /// documentation](crate::preview::request) for more details.
    ///
    /// This method is mainly useful for re-using the picker for multiple matches since the
    /// internal memory buffers used for matching and scoring are preserved. To restart the
    /// picker during interactive use, see the [`Event`] documentation or the [restart
    /// example](https://github.com/autobib/nucleo-picker/blob/master/examples/restart.rs).
    pub fn restart(&mut self) {
        self.core.engine.restart();
        self.core
            .list_state
            .layout
            .restart(self.core.engine.snapshot(), &self.core.list_state.config);
        self.update_query("");
    }

    /// Restart the match engine, disconnecting all active injectors and replacing the internal
    /// renderer.
    ///
    /// The provided [`Render`] implementation must be the same type as the one originally
    /// provided; this is most useful for stateful renderers.
    ///
    /// See [`Picker::restart`] for more detail. Note that method *does not* clear the query.
    #[inline]
    pub fn reset_renderer(&mut self, render: R) {
        self.core.engine.reset_renderer(render);
        self.core
            .list_state
            .layout
            .restart(self.core.engine.snapshot(), &self.core.list_state.config);
    }

    /// Get an [`Injector`] to send items to the picker.
    #[must_use]
    #[inline]
    pub fn injector(&self) -> Injector<T, R> {
        self.core.engine.injector()
    }

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// This method has been superseded by [`push_batch`](Self::push_batch).
    #[deprecated(
        since = "0.12.0",
        note = "Use `push_batch` instead which optimizes using `size_hint`"
    )]
    #[inline]
    pub fn extend_exact<U>(&self, iter: U)
    where
        R: Render<T>,
        U: IntoIterator<Item = T>,
        U::IntoIter: ExactSizeIterator,
    {
        self.injector().push_batch(iter);
    }

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// This is a convenience wrapper around [`Injector::push_batch`] for adding items synchronously
    /// before running the picker. Only use this method for a small number of items known in
    /// advance. In most cases, you want to use an [`Injector`] and send items to the picker from a
    /// separate thread.
    #[inline]
    pub fn push_batch<U>(&self, iter: U)
    where
        R: Render<T>,
        U: IntoIterator<Item = T>,
    {
        self.injector().push_batch(iter);
    }

    /// A convenience method to obtain the rendered version of an item as it would appear in the
    /// picker.
    ///
    /// This is the same as calling [`Render::render`] on the [`Render`] implementation internal
    /// to the picker.
    #[inline]
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a>
    where
        R: Render<T>,
    {
        self.core.engine.render(item)
    }
}

impl<T, R, I> Picker<T, R, I> {
    /// Change the interface of this picker.
    #[must_use]
    #[inline]
    pub fn with_interface<J: Interface<T>>(self, interface: J) -> Picker<T, R, J> {
        self.core.with_interface(interface)
    }

    /// Borrow this picker with a different interface.
    #[must_use]
    #[inline]
    pub fn as_interface<J: Interface<T>>(&mut self, interface: J) -> PickerRef<'_, T, R, J> {
        self.core.as_interface(interface)
    }
}

impl<T, R> Picker<T, R> {
    /// Get a picker variant which also generates previews.
    ///
    /// The original picker and the previewer can be recovered using
    /// [`Picker::into_parts`].
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[inline]
    pub fn with_preview<P>(self, previewer: P) -> Picker<T, R, WithPreview<Single, P>> {
        self.core.with_interface(WithPreview::new_single(previewer))
    }

    /// Convert into a multi-selection picker.
    #[must_use]
    #[inline]
    pub fn multi(self) -> Picker<T, R, Multi> {
        self.core.with_interface(Multi)
    }

    /// Mutably borrow this picker as a multi-selection picker.
    ///
    /// This is a convenience method for one-off multi-selection, for instance by chaining
    /// `picker.as_multi().pick()`.
    #[must_use]
    #[inline]
    pub fn as_multi(&mut self) -> PickerRef<'_, T, R, Multi> {
        self.core.as_interface(Multi)
    }
}

impl<T, R> Picker<T, R, Multi> {
    /// Get a picker variant which also generates previews.
    ///
    /// The original picker and the previewer can be recovered using
    /// [`Picker::into_parts`].
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    #[inline]
    pub fn with_preview<P>(self, previewer: P) -> Picker<T, R, WithPreview<Multi, P>> {
        self.core.with_interface(WithPreview::new_multi(previewer))
    }

    /// Convert into a single-selection picker.
    #[must_use]
    #[inline]
    pub fn single(self) -> Picker<T, R> {
        self.core.with_interface(Single)
    }

    /// Mutably borrow this picker as a single-selection picker.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> PickerRef<'_, T, R, Single> {
        self.core.as_interface(Single)
    }
}

impl<T: Send + Sync + 'static, R, I> Picker<T, R, I> {
    /// Filter the items sent to the picker using the current value of the query.
    ///
    /// This method requires that there are no live injectors (so that no new items can be added to
    /// the picker) and will return `None` if this is not the case. In order to receive updates
    /// when there are live injectors, use [`update_matches`](Picker::update_matches) instead.
    ///
    /// If there are no live injectors, this method is equivalent to polling [`update_matches`](Picker::update_matches) in a
    /// loop until matching is complete.
    ///
    /// # Examples
    ///
    /// ```
    /// use nucleo_picker::{PickerOptions, render::StrRenderer};
    ///
    /// let mut picker = PickerOptions::new().query("^blue$").picker(StrRenderer);
    /// picker.push_batch(["red", "blue", "green"]);
    /// assert_eq!(picker.filter().unwrap().get(0), Some(&"blue"));
    ///
    /// picker.reset_query();
    /// assert_eq!(picker.filter().unwrap().len(), 3);
    /// ```
    ///
    /// Automatically select a unique match, but open the interactive picker if there is ambiguity:
    ///
    /// ```no_run
    /// use nucleo_picker::{PickerOptions, render::StrRenderer};
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mut picker = PickerOptions::new().query("bl").picker(StrRenderer);
    /// picker.push_batch(["red", "blue", "black"]);
    /// let count = picker.filter().map(|matches| matches.len());
    /// let selected = match count {
    ///     Some(0) => None,
    ///     Some(1) => picker.matches().get(0),
    ///     _ => picker.pick()?,
    /// };
    /// if let Some(item) = selected {
    ///     println!("{item}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn filter(&mut self) -> Option<Matches<'_, T>> {
        let items_changed = self.core.engine.finish_matching()?;
        self.core.list_state.items_changed |= items_changed;
        Some(self.matches())
    }

    /// Advance the internal match engine state non-interactively.
    ///
    /// The `wait` duration is the approximate amount of time that this method will try to wait in
    /// order to synchronize with work in a background thread.
    ///
    /// This method synchronizes state between the background work, the picker, and injectors:
    /// calling this method is required to process new background items, to handle query updates,
    /// and to update the matches internal to this picker. Matching work which has already been
    /// dispatched will continue independently in the background.
    ///
    /// If [`MatchStatus::is_complete`] returns true, the snapshot includes every match for the current
    /// query and input. For a blocking convenience method, use [`filter`](Self::filter).
    pub fn update_matches(&mut self, wait: Duration) -> MatchStatus {
        let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        let status = self.core.engine.update(millis);
        self.core.list_state.items_changed |= status.items_changed;
        status
    }

    /// Returns a view of the current matches.
    ///
    /// This is the most up-to-date as of the last call to [`update_matches`](Self::update_matches).
    /// In particular, it can be out-of-date if matching is incomplete, if the query changes, or if
    /// there are new items. If there are no active injectors, you can also use
    /// [`filter`](Self::filter) to block until matching is complete.
    #[must_use]
    #[inline]
    pub fn matches(&self) -> Matches<'_, T> {
        Matches {
            snapshot: self.core.engine.snapshot(),
        }
    }
}

#[cfg(feature = "unstable-backend")]
#[doc(hidden)]
impl<T: Send + Sync + 'static, R, I> Picker<T, R, I> {
    /// Run the picker interactively with a custom event source and terminal backend.
    ///
    /// This is the same as [`pick_with_io`](Self::pick_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<I::Output<'_>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        I: Interface<T>,
        E: EventSource,
        W: Terminal,
    {
        self.interface.run(&mut self.core, event_source, terminal)
    }
}

/// Deprecated multi-selection methods. Use [`Picker::multi`] or [`Picker::as_multi`] instead.
impl<T: Send + Sync + 'static, R> Picker<T, R> {
    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`pick`](Self::pick). See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    #[deprecated(since = "0.13.0", note = "Use `as_multi().pick()` instead")]
    pub fn pick_multi(&mut self) -> Result<Selection<'_, T>, PickError>
    where
        R: Render<T>,
    {
        self.as_multi().pick()
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`pick_with_keybind`](Self::pick_with_keybind). See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    #[deprecated(
        since = "0.13.0",
        note = "Use `as_multi().pick_with_keybind(...)` instead"
    )]
    pub fn pick_multi_with_keybind<F>(&mut self, keybind: F) -> Result<Selection<'_, T>, PickError>
    where
        R: Render<T>,
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        self.as_multi().pick_with_keybind(keybind)
    }

    /// Run the picker interactively with a custom event source and writer, allowing the user to
    /// select multiple items.
    ///
    /// This is otherwise identical to [`pick_with_io`](Self::pick_with_io); see those docs as well
    /// as the [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[deprecated(since = "0.13.0", note = "Use `as_multi().pick_with_io(...)` instead")]
    #[inline]
    pub fn pick_multi_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Selection<'_, T>, PickError<<E as EventSource>::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: io::Write,
    {
        self.as_multi().pick_with_io(event_source, writer)
    }

    /// Run the picker interactively with a custom event source and terminal backend, allowing the
    /// user to select multiple items.
    ///
    /// This is the same as [`pick_multi_with_io`](Self::pick_multi_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[deprecated(
        since = "0.13.0",
        note = "Use `as_multi().pick_with_terminal_io(...)` instead"
    )]
    #[inline]
    pub fn pick_multi_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Selection<'_, T>, PickError<<E as EventSource>::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: Terminal,
    {
        self.as_multi()
            .pick_with_terminal_io(event_source, terminal)
    }
}
