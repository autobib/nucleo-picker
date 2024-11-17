//! # A generic fuzzy item picker
//! This is a generic picker implementation based on the [`nucleo::Nucleo`] matching engine. The
//! main feature is an interactive fuzzy picker TUI, similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf).
//!
//! In short, initialize a [`Picker`] using [`PickerOptions`] and describe how the items
//! should be represented by implementing [`Render`], or using a [built-in renderer](render).
//!
//! ## Example
//! For more usage examples, visit the [examples
//! folder](https://github.com/autobib/nucleo-picker/tree/master/examples) on GitHub.
//! ```no_run
#![doc = include_str!("../examples/find.rs")]
//! ```
mod bind;
mod component;
pub mod render;
mod term;

use std::{
    io,
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
    tty::IsTty,
};
use nucleo::{
    self as nc,
    pattern::{CaseMatching, Normalization},
    Nucleo,
};

pub use nucleo;

use crate::term::{EventSummary, PickerConfig, PickerState};

/// A trait which can render objects for matching and display.
///
/// Some renderers for common types are already implemented in the [`render`] module. In
/// particular, if render performance is not a bottleneck, the
/// [`DisplayRender`](render::DisplayRender) struct is particularly easy to use.
///
/// ## Example
/// Here is a basic example for how one would implement a renderer, even for a foreign type.
/// ```
/// use std::borrow::Cow;
///
/// use nucleo_picker::Render;
/// use ignore::DirEntry;
///
/// #[derive(Clone)]
/// pub struct DirEntryRender;
///
/// impl Render<DirEntry> for DirEntryRender {
///     type Column<'a> = Cow<'a, str>;
///
///     fn as_column<'a>(&mut self, value: &'a DirEntry) -> Self::Column<'a> {
///         value.path().to_string_lossy()
///     }
/// }
/// ```
/// ## Performance considations
/// Generally speaking, this crate assumes that the [`Render`] implementation is quite
/// fast. For each value, the [`Render`] implementation is first called to generate the match
/// objects, and then called again in order to render the interactive picker screen with the
/// relevant matches.
///
/// In particular, very slow [`Render`] implementations will reduce interactivitity of the terminal
/// interface. A crude rule of thumb is that rendering a single item should take (in the worst case)
/// at most 100μs. For comparison, formatting an `f64` into pre-allocated buffer takes around 0.1μs.
///
/// If this is not the case for your type, it is highly recommended to cache the render
/// computation. One possibility is to do this directly inside your type:
/// ```
/// # use nucleo_picker::Render;
/// pub struct Item<D> {
///     data: D,
///     column: String,
/// }
///
/// #[derive(Clone, Copy)]
/// pub struct ItemRender;
///
/// impl<D> Render<Item<D>> for ItemRender {
/// type Column<'a>
///     = &'a str
/// where
///     D: 'a;
///
///     fn as_column<'a>(&'a mut self, item: &'a Item<D>) -> Self::Column<'a> {
///         &item.column
///     }
/// }
/// ```
/// Note that the rendered column can borrow from `&self`. This means, for instance, that you can
/// write the representation of your type to an internal buffer and return a slice of that buffer.
///
/// ## Cloning
/// The [`Clone`] implementation is called each time the [`Picker::injector`] method is called.
/// As a result, if you expect to use many injectors, but do not expect to mutate your internal
/// state, you should internally use [`Arc`] for types for which [`Clone`] is very expensive.
///
pub trait Render<T>: Clone {
    /// The string type that `T` is rendered as.
    type Column<'a>: AsRef<str>
    where
        T: 'a,
        Self: 'a;

    /// Render the given value as a column in the picker.
    fn as_column<'a>(&'a mut self, value: &'a T) -> Self::Column<'a>;
}

/// A handle which allows adding new items to a [`Picker`].
#[derive(Clone)]
pub struct Injector<T, R> {
    inner: nc::Injector<T>,
    render: R,
}

impl<T, R: Render<T>> Injector<T, R> {
    /// Send a value to the matcher engine.
    pub fn push(&mut self, value: T) {
        self.inner.push(value, |s, columns| {
            columns[0] = self.render.as_column(s).as_ref().into();
        });
    }
}

/// # A builder for the picker
/// Specify configuration options for a [`Picker`] before initialization.
pub struct PickerOptions {
    _config: nc::Config,
    _query: Option<String>,
    _threads: Option<NonZero<usize>>,
    _picker_config: PickerConfig,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self {
            _config: nc::Config::DEFAULT,
            _query: None,
            _threads: None,
            _picker_config: PickerConfig::default(),
        }
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of threads used by the picker.
    ///
    /// If `None`, this will default to the number of available processors on your device
    /// minus 2, with a lower bound of 1.
    pub fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self._threads = threads;
        self
    }

    /// Set the internal matcher configuration.
    pub fn config(mut self, config: nc::Config) -> Self {
        self._config = config;
        self
    }

    /// Whether or not to highlight matches.
    pub fn highlight(mut self, highlight: bool) -> Self {
        self._picker_config.highlight = highlight;
        self
    }

    /// How much space to leave after rendering the rightmost highlight.
    pub fn right_highlight_buffer(mut self, size: u16) -> Self {
        self._picker_config.right_highlight_buffer = size;
        self
    }

    /// How to treat case mismatch.
    pub fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self._picker_config.case_matching = case_matching;
        self
    }

    /// How to perform Unicode normalization.
    pub fn normalization(mut self, normalization: Normalization) -> Self {
        self._picker_config.normalization = normalization;
        self
    }

    /// Provide a default query string.
    pub fn query<Q: ToString>(mut self, query: Q) -> Self {
        self._query = Some(query.to_string());
        self
    }

    /// Convert into a [`Picker`].
    pub fn picker<T: Send + Sync + 'static, R>(self, render: R) -> Picker<T, R> {
        let matcher = Nucleo::new(
            self._config.clone(),
            Arc::new(|| {}),
            // nucleo's API is a bit weird here in that it does not accept `NonZero<usize>`
            self._threads
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
            render,
            picker_config: self._picker_config,
            config: self._config,
            query: self._query,
        }
    }
}

/// A fuzzy matching interactive item picker.
///
/// The parameter `T` is the item type and the parameter `R` is the renderer, which describes how
/// to represent `T` in the matcher.
///
/// Initialize a picker with [`Picker::new`], or with custom configuration using
/// [`PickerOptions`], and add elements to the picker using a [`Injector`] returned
/// by the [`Picker::injector`] method.
///
/// See also the documentation for [`nucleo::Nucleo`] and [`Injector`], or the
/// [usage examples](https://github.com/autobib/nucleo-picker/tree/master/examples).
pub struct Picker<T: Send + Sync + 'static, R> {
    matcher: Nucleo<T>,
    render: R,
    picker_config: PickerConfig,
    config: nc::Config,
    query: Option<String>,
}

impl<T: Send + Sync + 'static, R: Render<T>> Picker<T, R> {
    /// Initialize a new picker with default configuration and the provided renderer.
    pub fn new(render: R) -> Self {
        PickerOptions::default().picker(render)
    }

    /// Default frame interval of 16ms, or ~60 FPS.
    const fn default_frame_interval() -> Duration {
        Duration::from_millis(16)
    }

    /// Update the default query string to a provided value. This is mainly useful for modifying the
    /// query string before re-using the [`Picker`].
    ///
    /// See also the [`PickerOptions::query`] method to set the query during initialization.
    pub fn update_query(&mut self, query: String) {
        self.query = Some(query);
    }

    /// Update the internal nucleo configuration.
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
    pub fn reset_render(&mut self, render: R) {
        self.restart();
        self.render = render;
    }

    /// Get a [`Injector`] wrapping a [`nucleo::Injector`] with a rendering implementation.
    ///
    /// # Performance considerations
    /// This clones the internal [`Render`] object. In most cases, this is either cheap of free:
    /// this is the case for all renderers implemented in the [`render`] module, which are
    /// zero-sized types. However, if you have a custom [`Render`] implementation which is
    /// expensive to clone, and you call the `injector` method a very large number of times, this
    /// may cause performance degredation.
    pub fn injector(&self) -> Injector<T, R> {
        Injector {
            inner: self.matcher.injector(),
            render: self.render.clone(),
        }
    }

    /// A convenience method to obtain the rendered version of a value as it would appear in the
    /// picker.
    pub fn render<'a>(&'a mut self, value: &'a T) -> <R as Render<T>>::Column<'a> {
        self.render.as_column(value)
    }

    /// Open the interactive picker prompt and return the picked item, if any.
    ///
    /// ## Custom [`io::Error`]
    /// This fails with an [`io::ErrorKind::Other`] if:
    ///
    /// 1. stderr is not interactive, in which the message will be `"is not interactive"`
    /// 2. the user presses `CTRL-C`, in which case the message will be `"keyboard interrupt"`
    ///
    /// ## Stderr lock
    /// The picker prompt is rendered in an alternate screen using the `stderr` file handle. In
    /// order to prevent screen corruption, a lock is acquired to `stderr`; see
    /// [`StderrLock`](std::io::StderrLock) for more detail.
    ///
    /// In particular, while the picker is interactive, any other thread which attempts to write to
    /// stderr will block. Note that `stdin` and `stdout` will remain fully interactive.
    pub fn pick(&mut self) -> Result<Option<&T>, io::Error> {
        if std::io::stderr().is_tty() {
            self.pick_inner(Self::default_frame_interval())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "is not interactive"))
        }
    }

    /// The actual picker implementation.
    fn pick_inner(&mut self, interval: Duration) -> Result<Option<&T>, io::Error> {
        let mut stderr = io::stderr().lock();
        let mut term = PickerState::new(size()?, &self.picker_config);
        let mut matcher = nucleo::Matcher::new(self.config.clone());
        if let Some(query) = self.query.as_ref() {
            term.set_prompt(query);
        }

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
                            .and_then(|idx| self.matcher.snapshot().get_matched_item(idx as _))
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
                &mut self.render,
                self.matcher.snapshot(),
            )?;

            // wait if frame rendering finishes early
            sleep(deadline - Instant::now());
        };

        disable_raw_mode()?;
        execute!(stderr, DisableBracketedPaste, LeaveAlternateScreen)?;
        selection
    }
}
