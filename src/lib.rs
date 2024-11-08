//! # A generic fuzzy item picker
//! This is a generic picker implementation which wraps the [`nucleo::Nucleo`] matching engine with
//! an interactive TUI.
//!
//! The API is pretty similar to that exposed by the [`nucleo`] crate; the majority of the internal state of [`Nucleo`] is re-exposed through the main [`Picker`] entrypoint.
//!
//! For usage examples, visit the [examples
//! folder](https://github.com/autobib/nucleo-picker/tree/master/examples) on GitHub.
mod bind;
pub mod component;
pub mod fill;
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
use nucleo::{Config, Injector, Nucleo};

pub use nucleo;

use crate::term::{EventSummary, PickerState};

/// # Options for a picker
/// Specify configuration options for a [`Picker`] before initialization.
pub struct PickerOptions {
    columns: u32,
    config: Config,
    query: Option<String>,
    threads: Option<NonZero<usize>>,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self {
            columns: 1,
            config: Config::DEFAULT,
            query: None,
            threads: None,
        }
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of columns.
    pub fn columns(&mut self, columns: u32) -> &mut Self {
        self.columns = columns;
        self
    }

    /// Set the number of threads used by the picker.
    ///
    /// If `None`, this will default to the number of available processors on your device
    /// minus 2, with a lower bound of 1.
    pub fn threads(&mut self, threads: Option<NonZero<usize>>) -> &mut Self {
        self.threads = threads;
        self
    }

    /// Set the internal matcher configuration.
    pub fn config(&mut self, config: Config) -> &mut Self {
        self.config = config;
        self
    }

    /// Provide a default query string.
    pub fn query<Q: ToString>(&mut self, query: Q) -> &mut Self {
        self.query = Some(query.to_string());
        self
    }

    /// Convert into a [`Picker`].
    pub fn picker<T: Send + Sync + 'static>(self) -> Picker<T> {
        let matcher = Nucleo::new(
            self.config,
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
            self.columns,
        );

        Picker {
            matcher,
            query: self.query,
        }
    }
}

/// # The core item picker
/// This is the main entrypoint for this crate. Initialize a picker with [`Picker::default`], or
/// with custom configuration using [`PickerOptions`], and add elements to the picker using an [`Injector`]
/// returned by the [`Picker::injector`] method.
///
/// See also the documentation for [`nucleo::Nucleo`] and [`nucleo::Injector`], or the
/// [usage examples](https://github.com/autobib/nucleo-picker/tree/master/examples).
pub struct Picker<T: Send + Sync + 'static> {
    matcher: Nucleo<T>,
    query: Option<String>,
}

impl<T: Send + Sync + 'static> Default for Picker<T> {
    fn default() -> Self {
        PickerOptions::default().picker()
    }
}

impl<T: Send + Sync + 'static> Picker<T> {
    /// Default frame interval of 16ms, or ~60 FPS.
    const fn default_frame_interval() -> Duration {
        Duration::from_millis(16)
    }

    /// Create a new [`Picker`] instance with arguments passed to [`Nucleo`].
    ///
    /// # Deprecated
    /// Configuration should be done using the [`PickerOptions`] struct instead.
    #[deprecated(since = "0.5.0", note = "Initialize using `PickerOptions` instead")]
    pub fn new(config: Config, num_threads: Option<usize>, columns: u32) -> Self {
        let mut opts = PickerOptions::new();
        let threads = num_threads.and_then(NonZero::<usize>::new);
        opts.config(config).threads(threads).columns(columns);
        opts.picker()
    }

    /// Convenience method to initialize a picker with all default settings, except with the provided
    /// nucleo [`Config`].
    pub fn with_config(config: Config) -> Self {
        let mut opts = PickerOptions::default();
        opts.config(config);
        opts.picker()
    }

    /// Update the default query string to a provided value. This is mainly useful for modifying the
    /// query string before re-using the [`Picker`].
    ///
    /// See also the [`PickerOptions::query`] method to set the query during initialization.
    pub fn update_query(&mut self, query: String) {
        self.query = Some(query);
    }

    /// Update the internal nucleo configuration.
    pub fn update_config(&mut self, config: Config) {
        self.matcher.update_config(config);
    }

    /// Restart the matcher engine, disconnecting all active injectors.
    ///
    /// Internally, this is a call to [`Nucleo::restart`] with `clear_snapshot = true`.
    /// See the documentation for [`Nucleo::restart`] for more detail.
    pub fn restart(&mut self) {
        self.matcher.restart(true);
    }

    /// Get an [`Injector`] from the internal [`Nucleo`] instance.
    pub fn injector(&self) -> Injector<T> {
        self.matcher.injector()
    }

    /// Open the interactive picker prompt and return the picked item, if any.
    ///
    /// This automatically fails with an [`io::ErrorKind::Other`] if either stdout or stdin is an
    /// interactive terminal. The picker will immediately abort without returning if `CTRL-C` is
    /// called during regular operation.
    pub fn pick(&mut self) -> Result<Option<&T>, io::Error> {
        if std::io::stdin().is_tty() && std::io::stdout().is_tty() {
            self.pick_inner(Self::default_frame_interval())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "is not interactive"))
        }
    }

    /// The actual picker implementation.
    fn pick_inner(&mut self, interval: Duration) -> Result<Option<&T>, io::Error> {
        let mut stdout = io::stdout();
        let mut term = PickerState::new(size()?);
        if let Some(query) = self.query.as_ref() {
            term.set_prompt(query);
        }

        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;

        let selection = loop {
            let deadline = Instant::now() + interval;

            // process any queued keyboard events and reset pattern if necessary
            match term.handle()? {
                EventSummary::Continue => {}
                EventSummary::UpdatePrompt(append) => {
                    self.matcher.pattern.reparse(
                        0,
                        &term.prompt_contents(),
                        nucleo::pattern::CaseMatching::Smart,
                        nucleo::pattern::Normalization::Smart,
                        append,
                    );
                }
                EventSummary::Select => {
                    break term
                        .selection()
                        .and_then(|idx| self.matcher.snapshot().get_matched_item(idx as _))
                        .map(|it| it.data);
                }
                EventSummary::Quit => {
                    break None;
                }
            };

            // increment the matcher and update state
            let status = self.matcher.tick(10);
            term.update(status.changed, self.matcher.snapshot());

            // redraw the screen
            term.draw(&mut stdout, self.matcher.snapshot())?;

            // wait if frame rendering finishes early
            sleep(deadline - Instant::now());
        };

        disable_raw_mode()?;
        execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen)?;
        Ok(selection)
    }
}
