mod options;

use std::time::Duration;

pub use options::DriverOptions;

use crate::{
    Injector, PickerChars, Render,
    interface::Interface,
    match_engine::MatchEngine,
    match_list::MatchListState,
    prompt::PromptState,
    session::{MatchStatus, Matches, Session},
};

/// Reusable matching and rendering machinery for running picker interfaces.
///
/// This is an externally driven match engine which contains the internal matching engine and query state.
///
/// Initialize with [`Self::new`] or [`DriverOptions::driver`]. In order to use this struct, there
/// are two options:
///
/// - Use headless methods (like [`Self::filter`]) to run matching only, independently of an
///   interface.
/// - Combine this struct with an [`Interface`] to start a session with [`Self::session`]. This
///   is for rendering your own picker interface, for instance as part of a larger application.
pub struct Driver<T, R> {
    pub(crate) engine: MatchEngine<T, R>,
    pub(crate) list_state: MatchListState,
    pub(crate) chars: PickerChars,
    pub(crate) prompt: PromptState,
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
    /// in order to run a picker. See the [`Session`] documentation for more details.
    ///
    /// This method does not perform any IO or match updates. For newly initialized drivers, if it
    /// is desirable for the first match list state to be up-to-date, call
    /// [`update_matches`](Self::update_matches).
    pub fn session<I: Interface<T>>(&mut self, interface: I) -> Session<'_, T, R, I>
    where
        R: Render<T>,
    {
        Session::new(
            &mut self.engine,
            &mut self.list_state,
            &mut self.prompt,
            &self.chars,
            interface,
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

    /// Return the current query, including edits made during interactive sessions.
    #[must_use]
    #[inline]
    pub fn query(&self) -> &str {
        self.prompt.contents()
    }

    /// Replace the query and reparse it for matching.
    #[inline]
    pub fn update_query<Q: Into<String>>(&mut self, query: Q) {
        self.prompt.set_query(query);
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
}
