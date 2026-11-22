mod options;

use std::{io, num::NonZero, time::Duration};

pub use options::DriverOptions;

#[cfg(doc)]
use crate::event::Event;
use crate::preview::PreviewConfig;
use crate::{
    Injector, PickerChars, Render,
    error::PickError,
    event::{EventSource, PickerStatus},
    interface::Interface,
    match_engine::MatchEngine,
    match_list::MatchListState,
    observer::{self, Notifier, Observer},
    prompt::PromptState,
    session::{MatchStatus, Matches, Session},
    terminal::CrosstermTerminal,
};

/// Reusable matching and rendering machinery for running picker interfaces.
///
/// Construct with [`Self::new`] or [`DriverOptions::driver`]. Items, query state, and matching
/// buffers persist between sessions. Interactive methods take an [`Interface`] explicitly;
/// their output borrows this driver independently of the interface.
///
/// Headless methods such as [`Self::filter`] can be used without an interface.
pub struct Driver<T, R> {
    pub(crate) engine: MatchEngine<T, R>,
    pub(crate) list_state: MatchListState,
    pub(crate) chars: PickerChars,
    pub(crate) prompt: PromptState,
    pub(crate) preview_config: PreviewConfig,
    pub(crate) interval: Duration,
    pub(crate) background_frame_frequency: NonZero<usize>,
    pub(crate) reversed: bool,
    pub(crate) restart_notifier: Option<Notifier<Injector<T, R>>>,
    pub(crate) status_notifier: Option<Notifier<PickerStatus>>,
}

impl<T: Send + Sync + 'static, R: Render<T>> Extend<T> for Driver<T, R> {
    #[inline]
    fn extend<U: IntoIterator<Item = T>>(&mut self, iter: U) {
        self.push_batch(iter);
    }
}

impl<T: Send + Sync + 'static, R> Driver<T, R> {
    /// Start a new interactive picker session.
    ///
    /// The session contains the given [`Interface`] and can be driven by the governing application
    /// in order to run a picker.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-local session entry point")
    )]
    pub(crate) fn session<I: Interface<T>>(
        &mut self,
        interface: I,
        startup_wait: Duration,
    ) -> Session<'_, T, R, I>
    where
        R: Render<T>,
    {
        Session::new(
            &mut self.engine,
            &mut self.list_state,
            &mut self.prompt,
            &self.chars,
            &self.preview_config,
            interface,
            startup_wait,
        )
    }

    /// Initialize a driver with default configuration and the provided renderer.
    #[must_use]
    #[inline]
    pub fn new(render: R) -> Self
    where
        R: Render<T>,
    {
        DriverOptions::new().driver(render)
    }

    /// Get an injector to send items to this driver.
    #[must_use]
    #[inline]
    pub fn injector(&self) -> Injector<T, R> {
        self.engine.injector()
    }

    /// Add a batch of items synchronously using [`Injector::push_batch`].
    #[inline]
    pub fn push_batch<U>(&self, iter: U)
    where
        R: Render<T>,
        U: IntoIterator<Item = T>,
    {
        self.injector().push_batch(iter);
    }

    /// Render an item using the internal renderer.
    #[inline]
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a>
    where
        R: Render<T>,
    {
        self.engine.render(item)
    }

    /// Run an interface with a custom event source and writer.
    ///
    /// Uses the terminal and error behavior described by
    /// [`Picker::pick_with_io`](crate::Picker::pick_with_io).
    #[inline]
    pub fn pick_with_io<I, E, W>(
        &mut self,
        interface: &mut I,
        event_source: E,
        writer: &mut W,
    ) -> Result<I::Output<'_>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        I: Interface<T>,
        E: EventSource,
        W: io::Write,
    {
        self.pick_impl(event_source, &mut CrosstermTerminal::new(writer), interface)
    }

    /// Run an interface with a custom event source and terminal backend.
    #[cfg(feature = "unstable-backend")]
    #[doc(hidden)]
    #[inline]
    pub fn pick_with_terminal_io<I, E, W>(
        &mut self,
        interface: &mut I,
        event_source: E,
        terminal: &mut W,
    ) -> Result<I::Output<'_>, PickError<E::AbortErr>>
    where
        R: Render<T>,
        I: Interface<T>,
        E: EventSource,
        W: crate::Terminal,
    {
        self.pick_impl(event_source, terminal, interface)
    }

    /// Return the current query, including edits made during interactive sessions.
    #[must_use]
    #[inline]
    pub fn query(&self) -> &str {
        self.prompt.contents()
    }

    /// Replace the query and reparse it for matching.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.prompt
            .set_query(query, crate::util::unicode::grapheme_width);
        self.engine.reparse(self.prompt.contents());
    }

    /// Reset the query to the empty string.
    #[inline]
    pub fn reset_query(&mut self) {
        self.update_query(String::new());
    }

    /// Remove all items, disconnect existing injectors, and clear the query.
    ///
    /// Matching buffers are retained for reuse. Existing item handles and injectors retain the
    /// previous item storage as described by [`Picker::restart`](crate::Picker::restart).
    pub fn restart(&mut self) {
        self.engine.restart();
        self.list_state
            .layout
            .restart(self.engine.snapshot(), &self.list_state.config);
        self.reset_query();
    }

    /// Restart matching with a replacement renderer, preserving the current query.
    #[inline]
    pub fn reset_renderer(&mut self, render: R) {
        self.engine.reset_renderer(render);
        self.list_state
            .layout
            .restart(self.engine.snapshot(), &self.list_state.config);
    }

    /// Finish matching against the current query, or return `None` if injectors are still live.
    ///
    /// Use [`Self::update_matches`] to process matches while items are still being injected.
    pub fn filter(&mut self) -> Option<Matches<'_, T>> {
        let items_changed = self.engine.finish_matching()?;
        self.list_state.items_changed |= items_changed;
        Some(self.matches())
    }

    /// Synchronize matching progress, waiting approximately `wait` for background work.
    ///
    /// The resulting matches are available through [`Self::matches`].
    pub fn update_matches(&mut self, wait: Duration) -> MatchStatus {
        let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        let status = self.engine.update(millis);
        self.list_state.items_changed |= status.items_changed;
        status
    }

    /// Return the matches available as of the last matching update.
    #[must_use]
    #[inline]
    pub fn matches(&self) -> Matches<'_, T> {
        Matches::new(&self.engine)
    }

    /// Observe replacement injectors produced by interactive restart events.
    ///
    /// Invalidates previous injector observers. If `with_injector` is true, initially contains an
    /// injector for the current generation. See [`Observer`] for channel and clone behavior.
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

    /// Observe responses to [`Event::Status`], invalidating previous status observers.
    ///
    /// See [`Observer`] for channel and clone behavior.
    #[must_use]
    #[inline]
    pub fn status_observer(&mut self) -> Observer<PickerStatus> {
        let (notifier, observer) = observer::channel();
        self.status_notifier = Some(notifier);
        observer
    }
}
