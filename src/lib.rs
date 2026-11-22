//! # A generic fuzzy item picker
//!
//! This crate contains a generic picker implementation that allows you to natively incorporate an
//! interactive fuzzy picker TUI (similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf)) directly in your own applications.
//!
//! In short, initialize a [`Picker`] using [`PickerOptions`] and describe how the items
//! should be represented by implementing [`Render`], or use a [built-in renderer](render).
//!
//! Here are some other common use-cases:
//!
//! - To include a preview pane in the picker, see the [`preview`] module. The simplest entry point is to
//!  enable the [preview-cache feature](#cargo-features) and use [`Picker::with_preview`].
//! - For headless (non-interactive) use, see the [relevant section of the picker documentation](Picker#headless-non-interactive-use).
//! - For more precise control over the interface and integration with an existing application, see the [`event`] module.
//! - See the [picker usage documentation](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md)
//!   for end-user-relevant TUI usage documentation.
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
//! | Feature | Default | Dependencies | Enables | Description |
//! | --- | --- | --- | --- | --- |
//! | <code style="white-space: nowrap;">unix-poll</code> | Yes | — | `crossterm/use-dev-tty` | Enables the <code style="white-space: nowrap;">use-dev-tty</code> feature of `crossterm` to fix a polling issue on macOS which breaks non-interactive STDIN. (Note that the <code style="white-space: nowrap;">use-dev-tty</code> feature is confusingly named, since even with this feature disabled, crossterm uses `/dev/tty` anyway.) |
//! | `preview-cache` | No | [`lru`](https://docs.rs/lru) | `nucleo-picker-interface/preview-cache` | Enables the [`Preview`](preview::Preview) trait and [`CachedPreview`]. |
//! | `serde` | No | [`serde`](https://docs.rs/serde) | `nucleo-picker-core/serde` | Adds a `DeserializeSeed` implementation for `&Injector`. |
//! | <code style="white-space: nowrap;">preview-ansi</code> | No | [`anstyle-parse`](https://docs.rs/anstyle-parse) | `nucleo-picker-core/preview-ansi` | Adds ANSI-parsing support for the preview buffer. |
//! | <code style="white-space: nowrap;">ratatui</code> | No | [`ratatui-core`](https://docs.rs/ratatui-core) | `nucleo-picker-core/ratatui` | Adds support for Ratatui styles and a convience widget API |

#![deny(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod error;
pub mod event;
mod frame;
mod input;
pub mod interface;
mod observer;
mod picker_ref;
pub mod preview;
mod rect;
pub mod render;
mod run;
#[cfg(test)]
mod session_tests;
mod terminal;
mod util;

use std::{io, iter::Extend, num::NonZero, time::Duration};

use crossterm::event::KeyEvent;
use observer::{Notifier, Observer};
use preview::BoundaryChars;

pub use nucleo_picker_core::{
    CaseMatching, Generation, Injector, Normalization, Render, draw, session,
};
pub use nucleo_picker_interface::Selection;
#[doc(inline)]
pub use picker_ref::PickerRef;

use crate::{
    error::PickError,
    event::{Event, EventSource, KeybindDefault, PickerStatus},
    interface::{Interface, Multi, Single},
    run::RunnerConfig,
    session::{MatchStatus, Matches, Model, ModelOptions},
    terminal::CrosstermTerminal,
};

#[cfg(feature = "preview-cache")]
use crate::interface::CachedPreview;

#[cfg(feature = "unstable-backend")]
pub use terminal::Terminal;
#[cfg(not(feature = "unstable-backend"))]
pub(crate) use terminal::Terminal;

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
            preview_cache_size: CachedPreview::<(), ()>::DEFAULT_CACHE_SIZE,
        }
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
    model: ModelOptions,
    runner: RunnerConfig,
    defaults: InterfaceDefaults,
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
            model: ModelOptions::new(),
            runner: RunnerConfig::new(),
            defaults: InterfaceDefaults::new(),
        }
    }

    /// Convert into a [`Picker`].
    #[must_use]
    pub fn picker<T: Send + Sync + 'static, R>(self, render: R) -> Picker<T, R> {
        Picker {
            core: self
                .model
                .list_reversed(self.runner.layout.reversed)
                .model(render),
            runner: self.runner,
            restart_notifier: None,
            status_notifier: None,
            interface: Single,
            defaults: self.defaults,
        }
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
        self.runner.layout.reversed = reversed;
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
        self.model.set_reverse_items(reversed);
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
        self.model.set_sort_results(sort);
        self
    }

    /// The maximum number of items that can be selected in the picker.
    ///
    /// If `None`, no bound is applied; otherwise, use the provided bound. This is a `u32` since
    /// the picker cannot hold more than [`u32::MAX`] items.
    ///
    /// The default is `None`.
    ///
    /// # Interface default
    ///
    /// This is the value propagated to into [`Multi`] by [`multi`](Picker::multi) and
    /// [`as_multi`](Picker::as_multi). Note that separately constructing the interface will ignore
    /// this option.
    #[must_use]
    #[inline]
    pub const fn max_selection_count(mut self, maximum: Option<NonZero<u32>>) -> Self {
        self.defaults.multi = self.defaults.multi.max_selection_count(maximum);
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
        self.runner.timing.interval = interval;
        self
    }

    /// Set the number of threads used by the internal matching engine.
    ///
    /// If `None` (default), use a heuristic choice based on the amount of available
    /// parallelism along with other factors.
    #[must_use]
    #[inline]
    pub const fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.model.set_threads(threads);
        self
    }

    /// How to perform Unicode normalization (defaults to [`Normalization::Smart`]).
    #[must_use]
    #[inline]
    pub const fn normalization(mut self, normalization: Normalization) -> Self {
        self.model.set_normalization(normalization);
        self
    }

    /// How to treat case mismatch (defaults to [`CaseMatching::Smart`]).
    #[must_use]
    #[inline]
    pub const fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.model.set_case_matching(case_matching);
        self
    }

    /// Enable score bonuses appropriate for matching file paths.
    #[must_use]
    #[inline]
    pub const fn match_paths(mut self) -> Self {
        self.model.set_match_paths();
        self
    }

    /// Whether to provide a bonus to matches by their distance from the start of the item.
    ///
    /// This is disabled by default and only recommended for autocompletion use-cases, where the expectation is that the user is typing the entire match.
    #[must_use]
    #[inline]
    pub const fn prefer_prefix(mut self, prefer_prefix: bool) -> Self {
        self.model.set_prefer_prefix(prefer_prefix);
        self
    }

    /// Whether or not to highlight matches (defaults to `true`).
    #[must_use]
    #[inline]
    pub const fn highlight(mut self, highlight: bool) -> Self {
        self.model.set_highlight(highlight);
        self
    }

    /// Whether to highlight the entire hovered item, and not only the match text
    ///
    /// This can be useful for distinguishing multiline items from each other. The default is false.
    #[must_use]
    #[inline]
    pub const fn highlight_line(mut self, highlight_line: bool) -> Self {
        self.model.set_highlight_line(highlight_line);
        self
    }

    /// How much space to leave when rendering match highlighting (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn highlight_padding(mut self, size: u16) -> Self {
        self.model.set_highlight_padding(size);
        self
    }

    /// How much space to leave around the hovered item when scrolling (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn scroll_padding(mut self, size: u16) -> Self {
        self.model.set_scroll_padding(size);
        self
    }

    /// How much space to leave around the cursor (defaults to `2`).
    #[must_use]
    #[inline]
    pub const fn prompt_padding(mut self, size: u16) -> Self {
        self.model.set_prompt_padding(size);
        self
    }

    /// Set the character used for the prompt indicator.
    ///
    /// The default value is `'>'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn prompt_indicator(mut self, prompt: char) -> Self {
        self.model.set_prompt_indicator(prompt);
        self
    }

    /// Set the character used to mark the hovered item.
    ///
    /// The default value is `'▌'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn hover_indicator(mut self, hover: char) -> Self {
        self.model.set_hover_indicator(hover);
        self
    }

    /// A deprecated alias for [`hover_indicator`](Self::hover_indicator).
    #[deprecated(since = "0.13.0", note = "Use `hover_indicator` instead")]
    #[must_use]
    #[inline]
    pub const fn selection_indicator(self, hover: char) -> Self {
        self.hover_indicator(hover)
    }

    /// Set the character used to indicate items queued for selection
    ///
    /// The default value is `'┃'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn queued_indicator(mut self, queued: char) -> Self {
        self.model.set_queued_indicator(queued);
        self
    }

    /// Set the character used to indicate text that has been truncated.
    ///
    /// The default value is `'…'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn truncation_ellipsis(mut self, ellipsis: char) -> Self {
        self.model.set_truncation_ellipsis(ellipsis);
        self
    }

    /// Set the character used for the horizontal rule separating the prompt and the match list.
    ///
    /// The default value is `'─'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn separator(mut self, sep: char) -> Self {
        self.model.set_separator(sep);
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
        self.model.set_spinner_chars(spinner);
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
        self.model.set_matching_indicator(indicator);
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
    ///     .hover_indicator('>')
    ///     .queued_indicator('|')
    ///     .truncation_ellipsis('.')
    ///     .separator('-')
    ///     .spinner_chars(&['|', '|', '/', '/', '-', '-', '\\', '\\'])
    ///     .matching_indicator('.');
    /// ```
    /// If `ascii` is `false`, this uses the default Unicode character set.
    pub const fn ascii_compatible(mut self, ascii: bool) -> Self {
        self.model.set_ascii_compatible(ascii);
        self.runner.preview_boundary_chars = if ascii {
            BoundaryChars::ascii()
        } else {
            BoundaryChars::new()
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
        self.runner.timing.background_frame_interval = interval;
        self
    }

    /// Provide an initial query string for the prompt (defaults to `""`).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.model.set_query(query);
        self
    }
}

impl PickerOptions {
    /// Set the preview size as a fraction of the total terminal width.
    ///
    /// This value must be finite and in the range `0..=1`.
    ///
    /// # Panics
    ///
    /// Values outside the range `0..=1` will cause a panic in debug builds, but in release builds
    /// the value will simply be set to `0.5`
    #[must_use]
    #[inline]
    pub const fn preview_split(mut self, ratio: f64) -> Self {
        self.runner.layout.set_preview_split(ratio);
        self
    }

    /// Set the preview boundary characters.
    ///
    /// This defaults to the value returned by [`BoundaryChars::new`]. Note that each character must
    /// have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn preview_boundary_chars(mut self, chars: BoundaryChars) -> Self {
        self.runner.preview_boundary_chars = chars;
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
    /// of 1 will lose the scroll position every time the hovered item changes.
    ///
    /// # Interface default
    ///
    /// This is the value propagated to into [`CachedPreview`] by
    /// [`with_preview`](Picker::with_preview). Note that separately constructing the interface will ignore
    /// this option.
    #[cfg(feature = "preview-cache")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
    #[must_use]
    #[inline]
    pub const fn preview_cache_size(mut self, size: Option<NonZero<usize>>) -> Self {
        self.defaults.preview_cache_size = size;
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
/// More generally, the picker is generic over its *interface* `I`, which in particular
/// defines the behaviour of the above 3 methods. Essentially, the parameter `I` controls the
/// following behaviour:
///
/// 1. Whether the picker operates in single selection mode (the default [`Single`]) or in multiple
///    selection mode ([`Multi`]).
/// 2. Whether the picker has item previews (the wrapper state [`CachedPreview`]).
///
/// For more details on picker variants read the [`Interface`] trait documentation and the read the
/// [interface module documentation](interface). In particular, jump to the sections on:
///
/// - [multiple selections](interface#multiple-selections)
/// - [adding previews](interface#adding-previews)
/// - [changing the interface](interface#changing-the-interface)
/// - [generics](interface#picker-generics)
///
/// ## Headless (non-interactive) use
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
///
/// Initializing a picker is a moderately expensive operation since the internal match engine uses
/// an arena-based memory approach to minimize allocator overhead during matching. This memory is
/// allocated when the picker is created.
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
///
/// ## Interface call order
///
/// The [`Picker`] drives an interface implementation in order to produce the interactive picker.
/// The picker drives the loop until [completion](#completion-and-errors) (i.e. if a selection is
/// made, the picker quits, or there is an error).
///
/// ### Initialization
///
/// When the session starts, the following call sequence is made.
///
/// 1. The match engine is updated with any changes which were processed in the background.
/// 2. [`init`](Interface::init) is called once. The preview pane is enabled according to [`PREVIEW`](Interface::PREVIEW)
///    and is dimensionless.
/// 3. The terminal is initialized.
/// 4. [`before_frame`](Interface::before_frame) is called with the initial hovered item.
///    Note that this is called before the component dimensions are computed; in general, the layout
///    information comes from the *previous* frame.
/// 5. [`queue_status`](Interface::queue_status) is called to update the status line.
/// 6. The first full frame is drawn. Immediately before drawing, terminal dimensions are queried
///    and resizes are propagated, for instance via [`preview_resize`](Interface::preview_resize)
///    (if the preview pane is enabled).
/// 7. [`after_frame`](Interface::after_frame) is called. The draw mask is used for the next frame.
///
/// ### Frame loop
///
/// On each frame:
///
/// 1. A bounded batch of events is processed. Events which cause the picker to exit are immediately
///    processed. This updates the prompt and match list, reparses if the query changed, and updates
///    the match engine. Depending on the precise events received, this calls [`on_queue`](Interface::on_queue), [`preview_event`](Interface::preview_event) and [`on_restart`](Interface::on_restart).
/// 2. [`before_frame`](Interface::before_frame) is called.
/// 3. [`queue_status`](Interface::queue_status) is called to update the status line.
/// 4. Check for pending redraws either from events or as required by the interface. If a resize is
///    required, a full redraw is done. If [`PREVIEW`](Interface::PREVIEW) is true,
///    [`preview_resize`](Interface::preview_resize) is called with the new dimensions (or `None` if the
///    preview pane is hidden).
/// 5. Draw the components. Internally, this calls [`is_queued`](Interface::is_queued) and
///    [`preview_content`](Interface::preview_content).
/// 6. If a status request can be processed, call [`queue_status`](Interface::queue_status).
/// 7. [`after_frame`](Interface::after_frame) is called. Its redraw mask is carried over to the next
///    frame.
///
/// Note that the frame hooks ([`before_frame`](Interface::before_frame) and
/// [`after_frame`](Interface::after_frame)) are run on all frames in which an error does not occur,
/// independently of whether or not changes were made.
///
/// ### Completion and errors
///
/// After an exit or error while processing events, the picker internally records any changes,
/// attempts to clean up the terminal, and then calls [`on_end`](Interface::on_end). If the exit
/// and cleanup succeeds, [`finish`](Interface::finish) is called.
///
/// Abnormal exit during panics will still result in attempted terminal cleanup but
/// will not result in usual interface cleanup methods from being called.
pub struct Picker<T, R, I = Single> {
    core: Model<T, R>,
    runner: RunnerConfig,
    restart_notifier: Option<Notifier<Injector<T, R>>>,
    status_notifier: Option<Notifier<PickerStatus>>,
    interface: I,
    defaults: InterfaceDefaults,
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
    /// Returns the generation of this picker.
    ///
    /// See the [`Generation`] docs for more detail.
    #[must_use]
    #[inline]
    pub fn generation(&self) -> Generation {
        self.core.generation()
    }

    /// Get an [`Injector`] to send items to the picker.
    #[must_use]
    #[inline]
    pub fn injector(&self) -> Injector<T, R> {
        self.core.injector()
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
        self.core.push_batch(iter);
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
        self.core.render(item)
    }

    /// Open the interactive picker prompt and return the picked item, if any.
    ///
    /// ## Interface
    ///
    /// The behaviour of this function is influenced by the [`Interface`] parameter (for instance,
    /// returning a single or multiple items; or rendering a preview). Read the [interface
    /// docs](Interface) for more details. Keybindings are provided by the [`KeybindDefault`]
    /// implementation.
    ///
    /// ## Stderr lock
    ///
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
    /// 2. [`PickError::UserInterrupted`] if the user presses `ctrl + c` (in the built-in
    ///    keybindings).
    ///
    /// This method will **never** return [`PickError::Disconnected`].
    #[inline]
    pub fn pick(&mut self) -> Result<I::Output<'_>, PickError>
    where
        R: Render<T>,
        I: Interface<T> + KeybindDefault,
    {
        input::pick(
            &mut self.core,
            &mut self.interface,
            self.runner,
            self.restart_notifier.as_ref(),
            self.status_notifier.as_ref(),
        )
    }

    /// Open the interactive picker prompt and return the picked item, if any. The provided
    /// keybindings are used in the interactive picker.
    ///
    /// The picker prompt is rendered in an alternate screen using the `stderr` file handle. See
    /// the [`pick`](Self::pick) method for more detail.
    ///
    /// To further customize event generation, see the [`pick_with_io`](Self::pick_with_io) method.
    /// The [`pick`](Self::pick) method is internally a call to this method with keybindings
    /// provided by [`KeybindDefault::keybind_default`].
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
        input::pick_with_keybind(
            &mut self.core,
            &mut self.interface,
            self.runner,
            self.restart_notifier.as_ref(),
            self.status_notifier.as_ref(),
            keybind,
        )
    }

    /// Run the picker interactively with a custom event source and writer, returning the selected
    /// items as determined by the interface.
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
    /// 4. If [`Event::Restart`] is processed without a live injector observer, the error returned
    ///    will be [`PickError::Disconnected`].
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
        run::pick_impl(
            &mut self.core,
            &mut self.interface,
            self.runner,
            self.restart_notifier.as_ref(),
            self.status_notifier.as_ref(),
            event_source,
            &mut CrosstermTerminal::new(writer),
        )
    }
}

impl<T, R> Picker<T, R> {
    /// Convert into a multi-selection picker.
    #[must_use]
    #[inline]
    pub fn multi(self) -> Picker<T, R, Multi> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface: self.defaults.multi,
            defaults: self.defaults,
        }
    }

    /// Mutably borrow this picker as a multi-selection picker.
    ///
    /// This is a convenience method for one-off multi-selection, for instance by chaining
    /// `picker.as_multi().pick()`.
    #[must_use]
    #[inline]
    pub fn as_multi(&mut self) -> PickerRef<'_, T, R, Multi> {
        PickerRef {
            core: &mut self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier.as_ref(),
            status_notifier: self.status_notifier.as_ref(),
            interface: self.defaults.multi,
            defaults: self.defaults,
        }
    }
}

impl<T, R> Picker<T, R, Multi> {
    /// Convert into a single-selection picker.
    #[must_use]
    #[inline]
    pub fn single(self) -> Picker<T, R> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface: Single,
            defaults: self.defaults,
        }
    }

    /// Mutably borrow this picker as a single-selection picker.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> PickerRef<'_, T, R, Single> {
        PickerRef {
            core: &mut self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier.as_ref(),
            status_notifier: self.status_notifier.as_ref(),
            interface: Single,
            defaults: self.defaults,
        }
    }
}

impl<T, R, I> Picker<T, R, I> {
    /// Get a picker variant which also generates previews.
    ///
    /// The original picker and the previewer can be recovered using [`Picker::into_parts`]. Note
    /// that this method can be called even if the interface already contains a previewer; this can
    /// create unnecessary overhead and should be avoided but will not result in incorrect
    /// behaviour.
    #[cfg(feature = "preview-cache")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
    #[inline]
    pub fn with_preview<P>(self, previewer: P) -> Picker<T, R, CachedPreview<I, P>> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface: CachedPreview::new(self.interface, previewer)
                .cache_size(self.defaults.preview_cache_size),
            defaults: self.defaults,
        }
    }

    /// Change the interface of this picker.
    #[must_use]
    #[inline]
    pub fn with_interface<J: Interface<T>>(self, interface: J) -> Picker<T, R, J> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface,
            defaults: self.defaults,
        }
    }

    /// Borrow this picker with a different interface.
    #[must_use]
    #[inline]
    pub fn as_interface<J: Interface<T>>(&mut self, interface: J) -> PickerRef<'_, T, R, J> {
        PickerRef {
            core: &mut self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier.as_ref(),
            status_notifier: self.status_notifier.as_ref(),
            interface,
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<T, R, P> Picker<T, R, CachedPreview<Single, P>> {
    /// Convert into a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn multi(self) -> Picker<T, R, CachedPreview<Multi, P>> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface: self.interface.with_interface(self.defaults.multi),
            defaults: self.defaults,
        }
    }

    /// Mutably borrow this picker as a multi-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_multi(&mut self) -> PickerRef<'_, T, R, CachedPreview<Multi, &mut P>> {
        PickerRef {
            core: &mut self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier.as_ref(),
            status_notifier: self.status_notifier.as_ref(),
            interface: self.interface.as_interface(self.defaults.multi),
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<T, R, P> Picker<T, R, CachedPreview<Multi, P>> {
    /// Convert back to a single-selection picker, retaining the internal previewer.
    #[must_use]
    #[inline]
    pub fn single(self) -> Picker<T, R, CachedPreview<Single, P>> {
        Picker {
            core: self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier,
            status_notifier: self.status_notifier,
            interface: self.interface.with_interface(Single),
            defaults: self.defaults,
        }
    }

    /// Mutably borrow this picker as a single-selection picker with item previews.
    #[must_use]
    #[inline]
    pub fn as_single(&mut self) -> PickerRef<'_, T, R, CachedPreview<Single, &mut P>> {
        PickerRef {
            core: &mut self.core,
            runner: self.runner,
            restart_notifier: self.restart_notifier.as_ref(),
            status_notifier: self.status_notifier.as_ref(),
            interface: self.interface.as_interface(Single),
            defaults: self.defaults,
        }
    }
}

#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
impl<T, R, I, P> Picker<T, R, CachedPreview<I, P>> {
    /// Convert into a picker without previews by dropping the previewer.
    ///
    /// This is convenient for stateless previewers, or for previewers passed by reference. To
    /// recover the internal previewer as well, use [`into_parts`](Self::into_parts).
    #[must_use]
    #[inline]
    pub fn without_preview(self) -> Picker<T, R, I> {
        self.into_parts().0
    }

    /// Decompose a previewing picker into the constituent picker and previewer.
    #[inline]
    pub fn into_parts(self) -> (Picker<T, R, I>, P) {
        let (interface, previewer) = self.interface.into_parts();
        (
            Picker {
                core: self.core,
                runner: self.runner,
                restart_notifier: self.restart_notifier,
                status_notifier: self.status_notifier,
                interface,
                defaults: self.defaults,
            },
            previewer,
        )
    }

    /// Returns a reference to the internal previewer.
    #[inline]
    pub fn previewer(&self) -> &P {
        self.interface.previewer()
    }

    /// Returns an exclusive reference to the internal previewer.
    #[inline]
    pub fn previewer_mut(&mut self) -> &mut P {
        self.interface.previewer_mut()
    }
}

impl<T: Send + Sync + 'static, R, I> Picker<T, R, I> {
    /// Returns the contents of the query string internal to the picker.
    ///
    /// If called after running [`Picker::pick`], this will contain the contents of the query string
    /// at the moment that the item was selected or the picker quit.
    #[must_use]
    #[inline]
    pub fn query(&self) -> &str {
        self.core.query()
    }

    /// Update the default query string. This is mainly useful for modifying the query string
    /// before re-using the [`Picker`].
    ///
    /// See the [`PickerOptions::query`] method to set the query during initialization, and
    /// [`PromptEvent::Reset`](event::PromptEvent::Reset) to reset the query during interactive
    /// use.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.core.update_query(query);
    }

    /// Reset the query string to the empty string. This is a convenience method to call
    /// [`update_query`](Self::update_query) with query string `""`.
    #[inline]
    pub fn reset_query(&mut self) {
        self.core.reset_query();
    }

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// All items are removed immediately. Existing injectors will continue to function but the
    /// items will no longer be received by this instance. The old items will only be dropped when
    /// all injectors are dropped.
    ///
    /// When previews are enabled, the old items will also only drop when all deferred preview
    /// requests (perhaps owned by the previewer) and [owned items](crate::session::OwnedItem) are dropped
    /// as well. See the [preview request documentation](crate::preview::request) for more details.
    ///
    /// This method is mainly useful for re-using the picker for multiple matches since the
    /// internal memory buffers used for matching and scoring are preserved. To restart the
    /// picker during interactive use, see the [`Event`] documentation or the [restart
    /// example](https://github.com/autobib/nucleo-picker/blob/master/examples/restart.rs).
    ///
    /// Also see the documentation for [`Generation`], which uniquely identifies when pickers and
    /// injectors are in correspondence.
    pub fn restart(&mut self) {
        self.core.restart();
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
        self.core.reset_renderer(render);
    }

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
    /// assert_eq!(picker.filter().unwrap().get(0).map(|item| item.value()), Some(&"blue"));
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
    ///     Some(1) => picker.matches().get(0).map(|item| item.value()),
    ///     _ => picker.pick()?,
    /// };
    /// if let Some(item) = selected {
    ///     println!("{item}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn filter(&mut self) -> Option<Matches<'_, T>> {
        self.core.filter()
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
        self.core.update_matches(wait)
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
        self.core.matches()
    }

    /// Returns an [`Observer`] containing up-to-date [`Injector`]s for this picker.
    ///
    /// This is the channel to which new injectors will be sent when the picker processes a
    /// [restart event](Event::Restart). Restart events are not generated by this library. You only
    /// need this channel if you generate restart events in your own code. See the [`Event`] documentation
    /// for more detail.
    ///
    /// Calling this method will *invalidate all earlier injector observers*. If you want multiple copies of
    /// the same observer, clone your existing observer. However, note that [cloning has
    /// caveats](Observer#clone-caveats): each status response can be witnessed by at most one
    /// observer clone. Callers with multiple consumers must coordinate receipt and distribution of
    /// responses.
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
    /// copies of the same observer, clone your existing observer. Note that [cloning has
    /// caveats](Observer#clone-caveats): each status response can be witnessed by at most one
    /// observer clone. Callers with multiple consumers must coordinate receipt and distribution of
    /// responses.
    #[must_use]
    #[inline]
    pub fn status_observer(&mut self) -> Observer<PickerStatus> {
        let (notifier, observer) = observer::channel();
        self.status_notifier = Some(notifier);
        observer
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
        run::pick_impl(
            &mut self.core,
            &mut self.interface,
            self.runner,
            self.restart_notifier.as_ref(),
            self.status_notifier.as_ref(),
            event_source,
            terminal,
        )
    }
}

impl<T: Send + Sync + 'static, R, I> Picker<T, R, I> {
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
}

/// Deprecated multi-selection methods. Use [`Picker::multi`] or [`Picker::as_multi`] instead.
impl<T: Send + Sync + 'static, R> Picker<T, R> {
    /// Open the interactive picker prompt and return the picked items, if any.
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
