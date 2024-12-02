//! # A generic fuzzy item picker
//! This is a generic picker implementation based on the [`nucleo::Nucleo`] matching engine. The
//! crate allows you to incorporate an interactive fuzzy picker TUI (similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf)) into your own applications.
//!
//! In short, initialize a [`Picker`] using [`PickerOptions`] and describe how the items
//! should be represented by implementing [`Render`], or use a [built-in renderer](render).
//!
//! ## Usage examples
//! For more usage examples, visit the [examples
//! folder](https://github.com/autobib/nucleo-picker/tree/master/examples) on GitHub.
//!
//! ### `fzf` example
//! Run this example with `cat myfile.txt | cargo run --release --example fzf`.
//! ```no_run
#![doc = include_str!("../examples/fzf.rs")]
//! ```
//!
//! ### `find` example
//! Run this example with `cargo run --release --example find ~`.
//! ```no_run
#![doc = include_str!("../examples/find.rs")]
//! ```

#![deny(missing_docs)]
#![warn(rustdoc::unescaped_backticks)]

mod bind;
mod component;
mod injector;
pub mod render;
mod term;

use std::{
    io::{self, IsTerminal},
    iter::Extend,
    num::NonZero,
    sync::Arc,
    thread::{available_parallelism, sleep},
    time::{Duration, Instant},
};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use nucleo::{
    self as nc,
    pattern::{CaseMatching, Normalization},
    Nucleo,
};

pub use nucleo;

pub use crate::injector::Injector;
use crate::{
    component::normalize_query_string,
    term::{Compositor, CompositorBuffer, EventSummary, PickerConfig},
};

/// A trait which describes how to render objects for matching and display.
///
/// Some renderers for common types are already implemented in the [`render`] module. In
/// many cases, the [`DisplayRenderer`](render::DisplayRenderer) is particularly easy to use.
///
/// Rendering *must* be **idempotent**: for a given render implementation `R` and a item `T`, the call
/// `R::render(&self, &T)` must depend only on the specific render instance and the specific item,
/// and not any other mutable state. Violation of this condition is normally only possible via
/// interior mutability, global state, I/O, or unsafe code.
///
/// If idempotence is violated, internal index computations which depend on the rendered format
/// will become invalid and the picker may either panic or return incorrect results. Note that such
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
/// The main exception is that tabs (`\t`) are not supported correctly: mainly, width calculations
/// will most likely be incorrect since the offset from printing a `\t` depends on the position
/// within the screen. In other words, a tab is more like a conditional cursor movement rather
/// than a printed character.
///
/// It is best to avoid tabs in your rendered format, and if you do not have control over the
/// incoming data and you suspect it will contain tabs, the most robust solution is likely to just
/// replace tabs with spaces.
/// ```
/// # use nucleo_picker::Render;
/// use std::borrow::Cow;
///
/// pub struct TabReplaceRenderer;
///
/// impl<T: AsRef<str>> Render<T> for TabReplaceRenderer {
///     type Str<'a>
///         = Cow<'a, str>
///     where
///         T: 'a;
///
///     fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
///         let item_ref = item.as_ref();
///
///         if item_ref.contains('\t') {
///             // replace tabs with two spaces
///             Cow::Owned(item_ref.replace('\t', "  "))
///         } else {
///             Cow::Borrowed(item_ref)
///         }
///     }
/// }
/// ```
///
/// ## Performance considations
/// Generally speaking, this crate assumes that the [`Render`] implementation is quite
/// fast. For each item, the [`Render`] implementation is first called to generate the match
/// objects, and then if the item is not ASCII, [`Render`] is called again in order to render
/// the interactive picker screen with the relevant matches.
///
/// In particular, very slow [`Render`] implementations which output non-ASCII will reduce
/// interactivity of the terminal interface. A crude rule of thumb is that rendering a single
/// item should take (in the worst case) at most 100μs. For comparison, display formatting an
/// `f64` takes less than 1μs.
///
/// If this is not the case for your type, it is highly recommended to cache the render
/// computation:
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

/// Specify configuration options for a [`Picker`].
///
/// Initialize with the [`new`](PickerOptions::new) function or (equivalently) the
/// [`Default`](PickerOptions::default) implementation, specify options, and then convert to e
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
pub struct PickerOptions {
    config: nc::Config,
    query: String,
    threads: Option<NonZero<usize>>,
    picker_config: PickerConfig,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self {
            config: nc::Config::DEFAULT,
            query: String::new(),
            threads: None,
            picker_config: PickerConfig::default(),
        }
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of threads used by the picker.
    ///
    /// If `None`, this will default to the number of available processors on your device
    /// minus 2, with a lower bound of 1.
    #[must_use]
    #[inline]
    pub fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.threads = threads;
        self
    }

    /// Set the internal matcher configuration.
    #[must_use]
    #[inline]
    pub fn config(mut self, config: nc::Config) -> Self {
        self.config = config;
        self
    }

    /// Whether or not to highlight matches.
    #[must_use]
    #[inline]
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.picker_config.highlight = highlight;
        self
    }

    /// How much space to leave after rendering the rightmost highlight.
    #[must_use]
    #[inline]
    pub fn right_highlight_padding(mut self, size: u16) -> Self {
        self.picker_config.right_highlight_padding = size;
        self
    }

    /// How much space to leave around the cursor when scrolling.
    #[must_use]
    #[inline]
    pub fn scroll_padding(mut self, size: u16) -> Self {
        self.picker_config.scroll_padding = size;
        self
    }

    /// How to treat case mismatch.
    #[must_use]
    #[inline]
    pub fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.picker_config.case_matching = case_matching;
        self
    }

    /// How to perform Unicode normalization.
    #[must_use]
    #[inline]
    pub fn normalization(mut self, normalization: Normalization) -> Self {
        self.picker_config.normalization = normalization;
        self
    }

    /// Provide a default query string.
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.query = query.into();
        normalize_query_string(&mut self.query);
        self
    }

    /// Convert into a [`Picker`].
    #[must_use]
    pub fn picker<T: Send + Sync + 'static, R>(self, render: R) -> Picker<T, R> {
        let matcher = Nucleo::new(
            self.config.clone(),
            Arc::new(|| {}),
            // nucleo's API is a bit weird here in that it does not accept `NonZero<usize>`
            self.threads
                .or_else(|| {
                    // Reserve two threads:
                    // 1. for populating the macher
                    // 2. for rendering the terminal UI and handling user input
                    available_parallelism()
                        .ok()
                        .and_then(|it| it.get().checked_sub(2).and_then(NonZero::new))
                })
                .map(NonZero::get),
            1,
        );

        Picker {
            matcher,
            render: render.into(),
            picker_config: self.picker_config,
            config: self.config,
            query: self.query,
        }
    }
}

/// A fuzzy matching interactive item picker.
///
/// The parameter `T` is the item type and the parameter `R` is the [renderer](Render), which describes how
/// to represent `T` in the matcher.
///
/// Initialize a picker with [`Picker::new`], or with custom configuration using
/// [`PickerOptions`], and add elements to the picker using a [`Injector`] returned
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
pub struct Picker<T: Send + Sync + 'static, R> {
    matcher: Nucleo<T>,
    render: Arc<R>,
    picker_config: PickerConfig,
    config: nc::Config,
    query: String,
}

impl<T: Send + Sync + 'static, R: Render<T>> Extend<T> for Picker<T, R> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let injector = self.injector();
        for it in iter {
            injector.push(it);
        }
    }
}

impl<T: Send + Sync + 'static, R: Render<T>> Picker<T, R> {
    /// Initialize a new picker with default configuration and the provided renderer.
    #[must_use]
    pub fn new(render: R) -> Self {
        PickerOptions::default().picker(render)
    }

    /// Default frame interval of 16ms, or ~60 FPS.
    const fn default_frame_interval() -> Duration {
        Duration::from_millis(16)
    }

    /// Update the default query string. This is mainly useful for modifying the query string
    /// before re-using the [`Picker`].
    ///
    /// See also the [`PickerOptions::query`] method to set the query during initialization.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.query = query.into();
        normalize_query_string(&mut self.query);
    }

    /// Update the internal nucleo configuration.
    #[inline]
    pub fn update_config(&mut self, config: nc::Config) {
        self.matcher.update_config(config);
    }

    /// Restart the matcher engine, disconnecting all active injectors.
    ///
    /// Internally, this is a call to [`Nucleo::restart`] with `clear_snapshot = true`.
    /// See the documentation for [`Nucleo::restart`] for more detail.
    pub fn restart(&mut self) {
        self.matcher.restart(true);
    }

    /// Restart the matcher engine, disconnecting all active injectors and replacing the internal
    /// renderer.
    ///
    /// See [`Picker::restart`] and [`Nucleo::restart`] for more detail.
    pub fn reset_renderer(&mut self, render: R) {
        self.restart();
        self.render = render.into();
    }

    /// Get an [`Injector`] to send items to the picker.
    #[must_use]
    pub fn injector(&self) -> Injector<T, R> {
        Injector::new(self.matcher.injector(), self.render.clone())
    }

    /// A convenience method to obtain the rendered version of an item as it would appear in the
    /// picker.
    ///
    /// This is the same as calling [`Render::render`] on the [`Render`] implementation internal
    /// to the picker.
    #[inline]
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a> {
        self.render.render(item)
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
    /// # Errors
    /// Underlying IO errors from the standard library or [`crossterm`] will be propogated.
    ///
    /// This fails with an [`io::ErrorKind::Other`] if:
    ///
    /// 1. stderr is not interactive, in which case the message will be `"is not interactive"`
    /// 2. the user presses `CTRL-C`, in which case the message will be `"keyboard interrupt"`
    pub fn pick(&mut self) -> Result<Option<&T>, io::Error> {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_inner(Self::default_frame_interval(), stderr)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "is not interactive"))
        }
    }

    /// The actual picker implementation.
    fn pick_inner(
        &mut self,
        interval: Duration,
        mut stderr: io::StderrLock<'_>,
    ) -> Result<Option<&T>, io::Error> {
        let mut term = Compositor::new(size()?, &self.picker_config);
        term.set_prompt(&self.query);

        let mut buffer = CompositorBuffer::new();
        let mut matcher = nucleo::Matcher::new(self.config.clone());

        enable_raw_mode()?;
        execute!(stderr, EnterAlternateScreen, EnableBracketedPaste)?;

        let selection = loop {
            let deadline = Instant::now() + interval;

            // process any queued keyboard events and reset pattern if necessary
            match term.handle() {
                Ok(summary) => match summary {
                    EventSummary::Continue => {}
                    EventSummary::UpdatePrompt(append) => {
                        self.matcher.pattern.reparse(
                            0,
                            &term.prompt_contents(),
                            self.picker_config.case_matching,
                            self.picker_config.normalization,
                            append,
                        );
                    }
                    EventSummary::Select => {
                        break Ok(term
                            .selection()
                            .try_into()
                            .ok()
                            .and_then(|idx| self.matcher.snapshot().get_matched_item(idx))
                            .map(|it| it.data));
                    }
                    EventSummary::Quit => {
                        break Ok(None);
                    }
                },
                // capture the internal error, so we can still attempt to clean up the terminal
                // afterwards
                Err(err) => break Err(err),
            };

            // increment the matcher and update state
            let status = self.matcher.tick(10);
            term.update(status.changed, self.matcher.snapshot());

            // redraw the screen
            term.draw(
                &mut stderr,
                &mut matcher,
                self.render.as_ref(),
                self.matcher.snapshot(),
                &mut buffer,
            )?;

            // wait if frame rendering finishes early
            sleep(deadline - Instant::now());
        };

        disable_raw_mode()?;
        execute!(stderr, DisableBracketedPaste, LeaveAlternateScreen)?;
        selection
    }
}
