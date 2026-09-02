//! # A generic fuzzy item picker
//! This crate contains a generic picker implementation that allows you to natively incorporate an
//! interactive fuzzy picker TUI (similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf)) directly in your own applications.
//!
//! In short, initialize a [`Picker`] using [`PickerOptions`] and describe how the items
//! should be represented by implementing [`Render`], or use a [built-in renderer](render).
//!
//! For more complex use-cases and integration with an existing application, see the
//! [`event`] module.
//!
//! ## Usage examples
//! For more usage examples, visit the [examples
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

#![deny(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod component;
pub mod error;
pub mod event;
mod frame;
mod incremental;
mod injector;
mod lazy;
mod match_list;
mod observer;
mod prompt;
pub mod render;
mod terminal;
mod util;

use std::{
    borrow::Cow,
    io::{self, BufWriter, IsTerminal},
    iter::Extend,
    num::NonZero,
    time::{Duration, Instant},
};

use crossterm::event::KeyEvent;
use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};
use observer::{Notifier, Observer};

use crate::{
    component::ComponentStatus,
    error::PickError,
    event::{
        Event, EventSource, PickerStatus, RecvError, StdinReader, keybind_default, keybind_no_multi,
    },
    frame::{FrameState, Redraw},
    lazy::{LazyMatchList, LazyPrompt},
    match_list::{MatchList, MatchListConfig, Queued, SelectedIndices},
    prompt::{Prompt, PromptConfig},
    terminal::{CrosstermTerminal, TerminalSession},
};

pub use crate::injector::Injector;
pub use crate::match_list::Selection;
pub use nucleo;

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
/// Rendering *must* be **pure**: for a given render implementation `R` and a item `T`, the call
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
/// ## Render considerations
/// The picker is capable of correctly displaying most Unicode data. Internally, Unicode width
/// calculations are performed to keep track of the amount of space that it takes on the screen to
/// display a given item.
///
/// The main exeption is control characters which are not newlines (`\n` or `\r\n`). Even visible
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
/// The **only** exception to this rule occurs the value returned by [`Render::render`] contains
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
    /// The default the Unicode character set.
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
    prompt_config: PromptConfig,
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
            prompt_config: PromptConfig::new(),
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

        let mut match_list = MatchList::new(
            self.match_list_config,
            self.config,
            nc::MatchListConfig {
                sort_results: self.sort_results,
                reverse_items: self.reverse_items,
            },
            self.threads,
            render.into(),
        );

        let mut prompt = Prompt::new(self.prompt_config);

        // set the prompt
        match_list.reparse(&self.query);
        prompt.set_query(self.query);

        Picker {
            match_list,
            prompt,
            interval: self.interval,
            background_frame_frequency,
            chars: self.chars,
            max_selection_count: self.max_selection_count,
            reversed,
            restart_notifier: None,
            status_notifier: None,
        }
    }

    /// Set 'reversed' layout.
    ///
    /// Option `false` (default) will put the prompt at the bottom and render items in ascending
    /// order. Option `true` will put the prompt at the top and render items in descending order.
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
    /// By default, the oldest items will appear at the beginning and the newest items will appear at the end.
    /// This can be swapped by setting [`reverse_items`](Self::reverse_items) to `true`.
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

    /// Set the internal match engine configuration (default to [`nucleo::Config::DEFAULT`]).
    #[must_use]
    #[inline]
    #[deprecated(
        since = "0.10.0",
        note = "Use native methods `prefer_prefix` and `match_paths`. The `normalize` and `ignore_case` settings are never used; use `normalization` and `case_matching` instead."
    )]
    pub fn config(mut self, config: nc::Config) -> Self {
        self.config = config;
        self
    }

    /// How to perform Unicode normalization (defaults to [`Normalization::Smart`]).
    #[must_use]
    #[inline]
    pub const fn normalization(mut self, normalization: Normalization) -> Self {
        self.match_list_config.normalization = normalization.convert();
        self
    }

    /// How to treat case mismatch (defaults to [`CaseMatching::Smart`]).
    #[must_use]
    #[inline]
    pub const fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.match_list_config.case_matching = case_matching.convert();
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

    /// Whether or not to highlight matches (default to `true`).
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

    /// How much space to leave when rendering match highlighting (default to `3`).
    #[must_use]
    #[inline]
    pub const fn highlight_padding(mut self, size: u16) -> Self {
        self.match_list_config.highlight_padding = size;
        self
    }

    /// How much space to leave around the selection when scrolling (default to `3`).
    #[must_use]
    #[inline]
    pub const fn scroll_padding(mut self, size: u16) -> Self {
        self.match_list_config.scroll_padding = size;
        self
    }

    /// How much space to leave around the cursor (default to `2`).
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
    /// must all Unicode width 1.
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
    pub const fn ascii_compatible(mut self, ascii: bool) -> Self {
        self.chars = if ascii {
            PickerChars::ascii()
        } else {
            PickerChars::new()
        };
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

    /// Provide an initial query string for the prompt (default to `""`).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.query = query.into();
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
/// The picker can be run in a number of different modes.
///
/// 1. The simplest (and most common) method is to use [`Picker::pick`].
/// 2. If you wish to customize keybindings, use [`Picker::pick_with_keybind`].
/// 3. If you wish to customize all IO to the picker, use [`Picker::pick_with_io`].
///
/// These methods return `Option<&T>` as the return type, where `None` indicates that no items were
/// selected.
///
/// ### Multiple selections
///
/// If you wish to permit the user to make multiple selections, use one of the similarly named
/// methods:
///
/// 1. [`Picker::pick_multi`]
/// 2. [`Picker::pick_multi_with_keybind`]
/// 3. [`Picker::pick_multi_with_io`]
///
/// These methods are analogous to their single-selection variants, except additional items can be
/// queued with the [`MatchListEvent::ToggleDown`](crate::event::MatchListEvent::ToggleDown) and
/// [`MatchListEvent::ToggleUp`](crate::event::MatchListEvent::ToggleUp) events. The [default
/// keybindings](keybind_default) bind these to `⇥` and `shift + ⇥` respectively. In this case, an
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
/// ### Emulate single selection using a multi-picker
///
/// It is possible to emulate single selection with one of the multi-picker methods by setting
/// [`PickerOptions::max_selection_count`] to `Some(1)`. This will force the resulting
/// [`Selection`] to contain either 0 or 1 element, and you can convert to `Option<&T>` by calling
/// `next` on the [iterator](Selection::iter).
///
/// Note that the picker interface will be slightly different: it is still possible to
/// queue at most one picked item using `⇥`. With the non-multi-pickers, it is not possible to
/// queue items at all.
///
/// ## A note on memory usage
/// Initializing a picker is a relatively expensive operation since the internal match engine uses
/// an arena-based memory approach to minimize allocator costs, and this memory is initialized when
/// the picker is created.
///
/// To re-use the picker without additional start-up costs, use [`Picker::restart`].
///
/// # Example
/// Run the picker on [`Stdout`](std::io::Stdout) with no interactivity checks, and quitting
/// gracefully on `ctrl + c`.
/// ```no_run
#[doc = include_str!("../examples/custom_io.rs")]
/// ```
pub struct Picker<T, R> {
    match_list: MatchList<T, R>,
    chars: PickerChars,
    max_selection_count: Option<NonZero<u32>>,
    prompt: Prompt,
    interval: Duration,
    background_frame_frequency: NonZero<usize>,
    reversed: bool,
    restart_notifier: Option<Notifier<Injector<T, R>>>,
    status_notifier: Option<Notifier<PickerStatus>>,
}

impl<T: Send + Sync + 'static, R: Render<T>> Extend<T> for Picker<T, R> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let injector = self.injector();
        for it in iter {
            injector.push(it);
        }
    }
}

impl<T: Send + Sync + 'static, R> Picker<T, R> {
    /// Initialize a new picker with default configuration and the provided renderer.
    #[must_use]
    pub fn new(render: R) -> Self
    where
        R: Render<T>,
    {
        PickerOptions::default().picker(render)
    }

    /// Update the default query string. This is mainly useful for modifying the query string
    /// before re-using the [`Picker`].
    ///
    /// See the [`PickerOptions::query`] method to set the query during initialization, and
    /// [`PromptEvent::Reset`](event::PromptEvent::Reset) to reset the query during interactive
    /// use.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.prompt.set_query(query);
        self.match_list.reparse(self.prompt.contents());
    }

    /// Returns the contents of the query string internal to the picker.
    ///
    /// If called after running `Picker::pick`, this will contain the contents of the query string
    /// at the moment that the item was selected or the picker quit.
    #[must_use]
    pub fn query(&self) -> &str {
        self.prompt.contents()
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
    /// If `with_injector` is `true`, the channel is intialized with an injector currently valid
    /// for the picker on creation.
    #[must_use]
    pub fn injector_observer(&mut self, with_injector: bool) -> Observer<Injector<T, R>> {
        let (notifier, observer) = if with_injector {
            observer::occupied_channel(self.injector())
        } else {
            observer::channel()
        };
        self.restart_notifier = Some(notifier);
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
    pub fn status_observer(&mut self) -> Observer<PickerStatus> {
        let (notifier, observer) = observer::channel();
        self.status_notifier = Some(notifier);
        observer
    }

    /// Update the internal nucleo configuration.
    #[inline]
    pub fn update_config(&mut self, config: nc::Config) {
        self.match_list.update_nucleo_config(config);
    }

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// All items are removed immediately. Existing injectors will continue to function but the
    /// items will no longer be received by this instance. The old items will only be dropped when
    /// all injectors are dropped.
    ///
    /// This method is mainly useful for re-using the picker for multiple matches since the
    /// internal memory buffers are preserved. To restart the picker during interactive use, see
    /// the [`Event`] documentation or the [restart
    /// example](https://github.com/autobib/nucleo-picker/blob/master/examples/restart.rs).
    pub fn restart(&mut self) {
        self.match_list.restart();
        self.update_query("");
    }

    /// Restart the match engine, disconnecting all active injectors and replacing the internal
    /// renderer.
    ///
    /// The provided [`Render`] implementation must be the same type as the one originally
    /// provided; this is most useful for stateful renderers.
    ///
    /// See [`Picker::restart`] for more detail. Note that method *does not* clear the query.
    pub fn reset_renderer(&mut self, render: R) {
        self.match_list.reset_renderer(render);
    }

    /// Get an [`Injector`] to send items to the picker.
    #[must_use]
    pub fn injector(&self) -> Injector<T, R> {
        self.match_list.injector()
    }

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// This method has been superceded by [`push_batch`](Self::push_batch).
    #[deprecated(
        since = "0.12.0",
        note = "Use `push_batch` instead which optimizes using `size_hint`"
    )]
    pub fn extend_exact<I>(&self, iter: I)
    where
        R: Render<T>,
        I: IntoIterator<Item = T>,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        self.injector().push_batch(iter);
    }

    /// A convenience method to add a batch of items directly to the picker.
    ///
    /// This is a convenience wrapper around [`Injector::push_batch`] for adding items synchronously
    /// before running the picker. Only use this method for a small number of items known in
    /// advance. In most cases, you want to use an [`Injector`] and send items to the picker from a
    /// separate thread.
    pub fn push_batch<I>(&self, iter: I)
    where
        R: Render<T>,
        I: IntoIterator<Item = T>,
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
        self.match_list.render(item)
    }

    /// Open the interactive picker prompt and return the picked item, if any.
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
    pub fn pick(&mut self) -> Result<Option<&T>, PickError>
    where
        R: Render<T>,
    {
        self.pick_with_keybind(keybind_no_multi)
    }

    /// Open the interactive picker prompt and return the picked items, if any.
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`pick`](Self::pick). See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    pub fn pick_multi(&mut self) -> Result<Selection<'_, T>, PickError>
    where
        R: Render<T>,
    {
        self.pick_multi_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// The picker prompt is rendered in an alternate screen using the `stderr` file handle. See
    /// the [`pick`](Self::pick) method for more detail.
    ///
    /// To further customize event generation, see the [`pick_with_io`](Self::pick_with_io) method.
    /// The [`pick`](Self::pick) method is internally a call to this method with keybindings
    /// provided by [`keybind_default`].
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
    ///
    /// This method will **never** return [`PickError::Disconnected`].
    #[inline]
    pub fn pick_with_keybind<F>(&mut self, keybind: F) -> Result<Option<&T>, PickError>
    where
        R: Render<T>,
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    ///
    /// This method permits the user to select multiple items, but is otherwise identical to [`pick_with_keybind`](Self::pick_with_keybind). See those docs as well as the
    /// [docs on multiple selections](Picker#multiple-selections) for more detail.
    #[inline]
    pub fn pick_multi_with_keybind<F>(&mut self, keybind: F) -> Result<Selection<'_, T>, PickError>
    where
        R: Render<T>,
        F: FnMut(KeyEvent) -> Option<Event>,
    {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_multi_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Run the picker interactively with a custom event source and writer.
    ///
    /// The picker is rendered using the given writer. In most situations, you want to check that
    /// the writer is interactive using, for instance, [`IsTerminal`]. The picker reads
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
    /// 1. If [`EventSource::recv_timeout`] fails with a [`RecvError::Disconnected`], the error
    ///    returned will be [`PickError::Disconnected`].
    /// 2. The error will be [`PickError::UserInterrupted`] if the [`Picker`] receives an
    ///    [`Event::UserInterrupt`].
    /// 3. The error will be [`PickError::Aborted`] if the [`Picker`] receives an
    ///    [`Event::Abort`].
    ///
    /// This method will **never** return [`PickError::NotInteractive`] since interactivity checks
    /// are not done.
    pub fn pick_with_io<E, W>(
        &mut self,
        event_source: E,
        writer: &mut W,
    ) -> Result<Option<&T>, PickError<<E as EventSource>::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: io::Write,
    {
        self.pick_impl::<_, _, ()>(event_source, &mut CrosstermTerminal::new(writer))
    }

    /// Run the picker interactively with a custom event source and writer, allowing the user to
    /// select multiple items.
    ///
    /// This is otherwise identical to [`pick_with_io`](Self::pick_with_io); see those docs as well
    /// as the [docs on multiple selections](Picker#multiple-selections) for more detail.
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
        self.pick_impl::<_, _, SelectedIndices>(event_source, &mut CrosstermTerminal::new(writer))
    }

    fn pick_impl<E, W, Q: Queued>(
        &mut self,
        mut event_source: E,
        writer: &mut W,
    ) -> Result<Q::Output<'_, T>, PickError<<E as EventSource>::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: Terminal,
    {
        let mut queued_items = Q::init(self.max_selection_count);
        let mut terminal = TerminalSession::new(writer);

        terminal.init()?;

        let mut frame_start = Instant::now();

        // render the first frame
        let update_status = self.match_list.update(5);
        let size = terminal.size()?;
        let mut frame_state = FrameState::new(size);
        frame_state.observe(&update_status);
        self.match_list.resize(frame_state.match_list_height());
        frame_state.render_frame(self, &mut terminal, Redraw::all(), &queued_items)?;

        let mut redraw = Redraw::default();
        let mut handle_status = None;

        let selection = 'selection: loop {
            let mut lazy_match_list = LazyMatchList::new(&mut self.match_list, &mut queued_items);
            let mut lazy_prompt = LazyPrompt::new(&mut self.prompt);

            // process new events, but do not exceed the frame interval
            'event: loop {
                match event_source.recv_timeout(frame_start + self.interval - Instant::now()) {
                    Ok(event) => match event {
                        Event::Prompt(prompt_event) => {
                            lazy_prompt.handle(prompt_event);
                        }
                        Event::MatchList(match_list_event) => {
                            lazy_match_list.handle(match_list_event);
                        }
                        Event::Redraw => {
                            redraw.set_all();
                        }
                        Event::Quit => {
                            break 'selection Ok(self.match_list.select_none(queued_items));
                        }
                        Event::QuitPromptEmpty => {
                            // watch out for buffered prompt events!
                            lazy_prompt.flush();
                            if lazy_prompt.is_empty() {
                                self.prompt.set_query("");
                                break 'selection Ok(self.match_list.select_none(queued_items));
                            }
                        }
                        Event::Select => {
                            if lazy_match_list.has_queued_items() {
                                break 'selection Ok(self.match_list.select_queued(queued_items));
                            }

                            if let Some(n) = lazy_match_list.selection() {
                                break 'selection Ok(self.match_list.select_one(queued_items, n));
                            }
                        }
                        Event::Status { id } => {
                            handle_status = Some(id);
                        }
                        Event::Restart => match self.restart_notifier {
                            Some(ref notifier) => {
                                if notifier.push(lazy_match_list.restart()).is_err() {
                                    break 'selection Err(PickError::Disconnected);
                                }
                                redraw.match_list = true;
                                redraw.match_status = true;
                            }
                            None => break 'selection Err(PickError::Disconnected),
                        },
                        Event::UserInterrupt => {
                            break 'selection Err(PickError::UserInterrupted);
                        }
                        Event::Abort(err) => {
                            break 'selection Err(PickError::Aborted(err));
                        }
                    },
                    Err(RecvError::Timeout) => break 'event,
                    Err(RecvError::Disconnected) => {
                        break 'selection Err(PickError::Disconnected);
                    }
                    Err(RecvError::IO(io_err)) => break 'selection Err(PickError::IO(io_err)),
                }
            }

            // we have to set 'frame_start' immediately after processing events, so that the
            // render time is also included
            frame_start = Instant::now();

            // clear out any buffered events
            let prompt_status = lazy_prompt.finish();
            let match_list_status = lazy_match_list.finish();

            // update draw status
            redraw.prompt |= prompt_status.needs_redraw();
            redraw.match_list |=
                match_list_status.selection_changed || match_list_status.queued_changed;
            redraw.match_status |= match_list_status.queued_changed;

            // check if the prompt changed: if so, reparse the match list
            if prompt_status.contents_changed {
                self.match_list.reparse(self.prompt.contents());
                redraw.match_list = true;
                redraw.match_status = true;
            }

            // update the item list
            let background_frame = frame_state.advance(self.background_frame_frequency);
            let update_status = self
                .match_list
                .update(2 * self.interval.as_millis() as u64 / 3);
            frame_state.observe(&update_status);
            redraw.match_list |= update_status.items_changed;
            redraw.match_status |= update_status.items_changed;
            if background_frame {
                redraw.match_status |= frame_state
                    .update_marker(self.chars.spinner_chars, self.chars.matching_indicator);
            }

            // process size changes and redraw the frame
            let changed = redraw.any_required();
            if changed {
                // note: we re-poll the size as late as possible instead of depending
                // on explicit 'Resize' events because a resize event might be up to
                // 1 frame late. however, we do make sure *not* to poll size on every
                // frame, in case it is a bit slow
                let size_change = frame_state.update_size(terminal.size()?);
                if size_change.is_changed() {
                    redraw.set_all();
                }
                if size_change.height_changed() {
                    self.match_list.resize(frame_state.match_list_height());
                }
                frame_state.render_frame(self, &mut terminal, redraw, &queued_items)?;
            }
            terminal.end_frame(changed)?;

            // handle status request
            if let Some(id) = handle_status.take()
                && let Some(ref notifier) = self.status_notifier
            {
                let (width, height) = frame_state.dimensions();
                let status = PickerStatus {
                    id,
                    query: self.query().to_owned(),
                    changed,
                    selection: self.match_list.selection(),
                    item_count: self.match_list.item_count(),
                    selected_item_count: queued_items.len(),
                    matched_item_count: self.match_list.matched_item_count(),
                    width,
                    height,
                    matching: update_status.matching,
                    injecting: update_status.injecting,
                };
                let _ = notifier.push(status);
            }

            // reset the redraw markers
            redraw.reset();
        };

        terminal.finish()?;
        selection
    }
}

#[cfg(feature = "unstable-backend")]
#[doc(hidden)]
impl<T: Send + Sync + 'static, R> Picker<T, R> {
    /// Run the picker interactively with a custom event source and terminal backend.
    ///
    /// This is the same as [`pick_with_io`](Self::pick_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
    pub fn pick_with_terminal_io<E, W>(
        &mut self,
        event_source: E,
        terminal: &mut W,
    ) -> Result<Option<&T>, PickError<<E as EventSource>::AbortErr>>
    where
        R: Render<T>,
        E: EventSource,
        W: Terminal,
    {
        self.pick_impl::<_, _, ()>(event_source, terminal)
    }

    /// Run the picker interactively with a custom event source and terminal backend, allowing the
    /// user to select multiple items.
    ///
    /// This is the same as [`pick_multi_with_io`](Self::pick_multi_with_io) but in addition defers
    /// terminal-specific implementation to the passed [`Terminal`].
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
        self.pick_impl::<_, _, SelectedIndices>(event_source, terminal)
    }
}
