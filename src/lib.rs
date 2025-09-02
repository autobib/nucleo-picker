//! # A generic fuzzy item picker
//! This is a generic picker implementation based on the [`nucleo::Nucleo`] matching engine. The
//! crate allows you to incorporate an interactive fuzzy picker TUI (similar in spirit to the very popular
//! [fzf](https://github.com/junegunn/fzf)) into your own applications.
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

mod component;
pub mod error;
pub mod event;
mod incremental;
mod injector;
mod lazy;
mod match_list;
mod observer;
mod prompt;
pub mod render;
mod util;

use std::{
    borrow::Cow,
    io::{self, BufWriter, IsTerminal, Write},
    iter::Extend,
    num::NonZero,
    panic::{set_hook, take_hook},
    sync::Arc,
    thread::available_parallelism,
    time::{Duration, Instant},
};

use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::MoveTo,
    event::{DisableBracketedPaste, EnableBracketedPaste, KeyEvent},
    execute,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode, size,
    },
};
use nucleo::{
    self as nc, Nucleo,
    pattern::{CaseMatching, Normalization},
};
use observer::{Notifier, Observer};

use crate::{
    component::{Component, Status},
    error::PickError,
    event::{Event, EventSource, RecvError, StdinReader, keybind_default},
    lazy::{LazyMatchList, LazyPrompt},
    match_list::{MatchList, MatchListConfig},
    prompt::{Prompt, PromptConfig},
};

pub use crate::injector::Injector;
pub use nucleo;

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
/// control characters, such as tabs (`\t`) will cause issues: width calculations will most likely
/// be incorrect since the amount of space a tab occupies depends on its position within the
/// screen.
///
/// It is best to avoid such characters in your rendered format. If you do not have control
/// over the incoming data, the most robust solution is likely to perform substitutions during
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
pub struct PickerOptions {
    config: nc::Config,
    query: String,
    threads: Option<NonZero<usize>>,
    interval: Duration,
    match_list_config: MatchListConfig,
    prompt_config: PromptConfig,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self {
            config: nc::Config::DEFAULT,
            query: String::new(),
            threads: None,
            interval: Duration::from_millis(15),
            match_list_config: MatchListConfig::default(),
            prompt_config: PromptConfig::default(),
        }
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    ///
    /// Equivalent to the [`Default`] implementation.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert into a [`Picker`].
    #[must_use]
    pub fn picker<T: Send + Sync + 'static, R: Render<T>>(self, render: R) -> Picker<T, R> {
        let engine = Nucleo::new(
            self.config.clone(),
            Arc::new(|| {}),
            // nucleo's API is a bit weird here in that it does not accept `NonZero<usize>`
            self.threads
                .or_else(|| {
                    // Reserve two threads:
                    // 1. for populating the matcher
                    // 2. for rendering the terminal UI and handling user input
                    available_parallelism()
                        .ok()
                        .and_then(|it| it.get().checked_sub(2).and_then(NonZero::new))
                })
                .map(NonZero::get),
            1,
        );

        let reversed = self.match_list_config.reversed;

        let mut match_list =
            MatchList::new(self.match_list_config, self.config, engine, render.into());

        let mut prompt = Prompt::new(self.prompt_config);

        // set the prompt
        match_list.reparse(&self.query);
        prompt.set_query(self.query);

        Picker {
            match_list,
            prompt,
            interval: self.interval,
            reversed,
            restart_notifier: None,
        }
    }

    /// Set 'reversed' layout.
    ///
    /// Option `false` (default) will put the prompt at the bottom and render items in ascending
    /// order. Option `true` will put the prompt at the top and render items in descending order.
    #[must_use]
    #[inline]
    pub fn reversed(mut self, reversed: bool) -> Self {
        self.match_list_config.reversed = reversed;
        self
    }

    /// Set how long each frame should last.
    ///
    /// This is the reciprocal of the refresh rate. The default value is
    /// `Duration::from_millis(15)`, which corresponds to a refresh rate of approximately 67 frames
    /// per second. It is not recommended to set this to a value less than 8ms.
    #[must_use]
    #[inline]
    pub fn frame_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Set the number of threads used by the internal matching engine.
    ///
    /// If `None` (default), use a heuristic choice based on the amount of available
    /// parallelism along with other factors.
    #[must_use]
    #[inline]
    pub fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.threads = threads;
        self
    }

    /// Set the internal match engine configuration (default to [`nucleo::Config::DEFAULT`]).
    #[must_use]
    #[inline]
    pub fn config(mut self, config: nc::Config) -> Self {
        self.config = config;
        self
    }

    /// Whether or not to highlight matches (default to `true`).
    #[must_use]
    #[inline]
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.match_list_config.highlight = highlight;
        self
    }

    /// How much space to leave when rendering match highlighting (default to `3`).
    #[must_use]
    #[inline]
    pub fn highlight_padding(mut self, size: u16) -> Self {
        self.match_list_config.highlight_padding = size;
        self
    }

    /// How much space to leave around the selection when scrolling (default to `3`).
    #[must_use]
    #[inline]
    pub fn scroll_padding(mut self, size: u16) -> Self {
        self.match_list_config.scroll_padding = size;
        self
    }

    /// How much space to leave around the cursor (default to `2`).
    #[must_use]
    #[inline]
    pub fn prompt_padding(mut self, size: u16) -> Self {
        self.prompt_config.padding = size;
        self
    }

    /// How to treat case mismatch (default to [`CaseMatching::default`]).
    #[must_use]
    #[inline]
    pub fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.match_list_config.case_matching = case_matching;
        self
    }

    /// How to perform Unicode normalization (default to [`Normalization::default`]).
    #[must_use]
    #[inline]
    pub fn normalization(mut self, normalization: Normalization) -> Self {
        self.match_list_config.normalization = normalization;
        self
    }

    /// Provide an initial query string for the prompt (default to `""`).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.query = query.into();
        self
    }

    /// How much space to leave after rendering the rightmost highlight.
    #[must_use]
    #[deprecated(
        since = "0.6.2",
        note = "method has been renamed to `highlight_padding`"
    )]
    pub fn right_highlight_padding(mut self, size: u16) -> Self {
        self.match_list_config.highlight_padding = size;
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
/// The picker can be run in a number of different modes.
///
/// 1. The simplest (and most common) method is to use [`Picker::pick`].
/// 2. If you wish to customize keybindings, use [`Picker::pick_with_keybind`].
/// 3. If you wish to customize all IO to the picker, use [`Picker::pick_with_io`].
///
/// ## A note on memory usage
/// Initializing a picker is a relatively expensive operation since the internal match engine uses
/// an arena-based memory approach to minimize allocator costs, and this memory is initialized when
/// the picker is created.
///
/// To re-use the picker without additional start-up costs, use the [`Picker::restart`] method.
///
/// # Example
/// Run the picker on [`Stdout`](std::io::Stdout) with no interactivity checks, and quitting
/// gracefully on `ctrl + c`.
/// ```no_run
#[doc = include_str!("../examples/custom_io.rs")]
/// ```
pub struct Picker<T: Send + Sync + 'static, R> {
    match_list: MatchList<T, R>,
    prompt: Prompt,
    interval: Duration,
    reversed: bool,
    restart_notifier: Option<Notifier<Injector<T, R>>>,
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

    /// Returns an [`Observer`] containing up-to-date [`Injector`]s for this picker. For example,
    /// this is the channel to which new injectors will be sent when the picker processes a
    /// [restart event](Event::Restart). See the [`Event`] documentation for more detail.
    ///
    /// Restart events are not generated by this library. You only need this channel if you
    /// generate restart events in your own code.
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

    /// Update the internal nucleo configuration.
    #[inline]
    pub fn update_config(&mut self, config: nc::Config) {
        self.match_list.update_nucleo_config(config);
    }

    /// Restart the match engine, disconnecting all active injectors and clearing the existing
    /// search query.
    ///
    /// Internally, this is a call to [`Nucleo::restart`] with `clear_snapshot = true`.
    /// See the documentation for [`Nucleo::restart`] for more detail.
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
    /// See [`Picker::restart`] and [`Nucleo::restart`] for more detail.
    pub fn reset_renderer(&mut self, render: R) {
        self.match_list.reset_renderer(render);
    }

    /// Get an [`Injector`] to send items to the picker.
    #[must_use]
    pub fn injector(&self) -> Injector<T, R> {
        self.match_list.injector()
    }

    /// A convenience method to obtain the rendered version of an item as it would appear in the
    /// picker.
    ///
    /// This is the same as calling [`Render::render`] on the [`Render`] implementation internal
    /// to the picker.
    #[inline]
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a> {
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
    pub fn pick(&mut self) -> Result<Option<&T>, PickError> {
        self.pick_with_keybind(keybind_default)
    }

    /// Open the interactive picker prompt and return the picked item, if any. Uses the provided
    /// keybindings for the interactive picker.
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
    pub fn pick_with_keybind<F: FnMut(KeyEvent) -> Option<Event>>(
        &mut self,
        keybind: F,
    ) -> Result<Option<&T>, PickError> {
        let stderr = io::stderr().lock();
        if stderr.is_terminal() {
            self.pick_with_io(StdinReader::new(keybind), &mut BufWriter::new(stderr))
        } else {
            Err(PickError::NotInteractive)
        }
    }

    /// Initialize the alternate screen.
    #[inline]
    fn init_screen<W: Write>(writer: &mut W) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(writer, EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(())
    }

    /// Cleanup the alternate screen when finished.
    #[inline]
    fn cleanup_screen<W: Write>(writer: &mut W) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(writer, DisableBracketedPaste, LeaveAlternateScreen)?;
        Ok(())
    }

    /// Render the frame, specifying which parts of the frame need to be re-drawn.
    #[inline]
    fn render_frame<W: Write>(
        &mut self,
        writer: &mut W,
        redraw_prompt: bool,
        redraw_match_list: bool,
    ) -> io::Result<()> {
        let (width, height) = size()?;

        let (prompt_row, match_list_row) = if self.reversed {
            (0, 1)
        } else {
            (height - 1, 0)
        };

        if width >= 1 && (redraw_prompt || redraw_match_list) {
            writer.execute(BeginSynchronizedUpdate)?;

            if redraw_match_list && height >= 2 {
                writer.queue(MoveTo(0, match_list_row))?;

                self.match_list.draw(width, height - 1, writer)?;
            }

            if redraw_prompt && height >= 1 {
                writer.queue(MoveTo(0, prompt_row))?;

                self.prompt.draw(width, 1, writer)?;
            }

            writer.queue(MoveTo(self.prompt.screen_offset() + 2, prompt_row))?;

            // flush to terminal
            writer.flush()?;
            writer.execute(EndSynchronizedUpdate)?;
        };

        Ok(())
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
        mut event_source: E,
        writer: &mut W,
    ) -> Result<Option<&T>, PickError<<E as EventSource>::AbortErr>>
    where
        E: EventSource,
        W: io::Write,
    {
        // set panic hook in case the `Render` implementation panics
        let original_hook = take_hook();
        set_hook(Box::new(move |panic_info| {
            // intentionally ignore errors here since we're already panicking
            let _ = Self::cleanup_screen(&mut io::stderr());
            original_hook(panic_info);
        }));

        Self::init_screen(writer)?;

        let mut frame_start = Instant::now();

        // render the first frame
        self.match_list.update(5);
        self.render_frame(writer, true, true)?;

        let mut redraw_prompt = false;
        let mut redraw_match_list = false;

        let selection = 'selection: loop {
            let mut lazy_match_list = LazyMatchList::new(&mut self.match_list);
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
                            redraw_prompt = true;
                            redraw_match_list = true;
                        }
                        Event::Quit => {
                            break 'selection Ok(None);
                        }
                        Event::QuitPromptEmpty => {
                            if lazy_prompt.is_empty() {
                                break 'selection Ok(None);
                            }
                        }
                        Event::Select => {
                            // TODO: workaround for the borrow checker not understanding that
                            // the `None` variant does not borrow from the `match_list`
                            //
                            // maybe works when polonius is merged
                            if !lazy_match_list.is_empty() {
                                // the cursor may have moved
                                let n = lazy_match_list.selection();
                                let item = self.match_list.get_item(n).unwrap();
                                break 'selection Ok(Some(item.data));
                            }
                        }
                        Event::Restart => match self.restart_notifier {
                            Some(ref notifier) => {
                                if notifier.push(lazy_match_list.restart()).is_err() {
                                    break 'selection Err(PickError::Disconnected);
                                } else {
                                    redraw_match_list = true;
                                }
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
            redraw_prompt |= prompt_status.needs_redraw();
            redraw_match_list |= match_list_status.needs_redraw();

            // check if the prompt changed: if so, reparse the match list
            if prompt_status.contents_changed {
                self.match_list.reparse(self.prompt.contents());
                redraw_match_list = true;
            }

            // update the item list
            redraw_match_list |= self
                .match_list
                .update(2 * self.interval.as_millis() as u64 / 3)
                .needs_redraw();

            // render the frame
            self.render_frame(writer, redraw_prompt, redraw_match_list)?;

            // reset the redraw markers
            redraw_prompt = false;
            redraw_match_list = false;
        };

        Self::cleanup_screen(writer)?;
        selection
    }
}
