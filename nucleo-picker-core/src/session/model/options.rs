use std::num::NonZero;

use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};

use super::Model;
use crate::{
    CaseMatching, Normalization, PickerChars,
    match_engine::MatchEngine,
    match_list::{MatchListConfig, MatchListState},
    prompt::{PromptConfig, PromptState},
};

/// Configuration for a [`Model`].
pub struct ModelOptions {
    // PickerOptions updates fields in place: const evaluation rejects consuming a nested
    // value with drop glue, even when all of its fields are moved into the result.
    config: nc::Config,
    query: String,
    threads: Option<NonZero<usize>>,
    chars: PickerChars,
    match_list_config: MatchListConfig,
    case_matching: NucleoCaseMatching,
    normalization: NucleoNormalization,
    prompt_config: PromptConfig,
    sort_results: bool,
    reverse_items: bool,
}

impl Default for ModelOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelOptions {
    /// Initialize with default configuration.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            config: nc::Config::DEFAULT,
            query: String::new(),
            threads: None,
            chars: PickerChars::new(),
            match_list_config: MatchListConfig::new(),
            case_matching: NucleoCaseMatching::Smart,
            normalization: NucleoNormalization::Smart,
            prompt_config: PromptConfig::new(),
            sort_results: true,
            reverse_items: false,
        }
    }

    /// Construct a picker model with this configuration and the provided renderer.
    #[must_use]
    pub fn model<T: Send + Sync + 'static, R>(self, render: R) -> Model<T, R> {
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

        Model {
            engine,
            list_state,
            prompt,
            chars: self.chars,
        }
    }

    /// Set a 'reversed' rendering order for the match list.
    ///
    /// Option `false` (default) renders the items in the match list from the bottom to the top.
    #[must_use]
    #[inline]
    pub const fn list_reversed(mut self, reversed: bool) -> Self {
        self.set_list_reversed(reversed);
        self
    }

    /// In-place version of [`Self::list_reversed`].
    #[inline]
    pub const fn set_list_reversed(&mut self, reversed: bool) {
        self.match_list_config.reversed = reversed;
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
        self.set_reverse_items(reversed);
        self
    }

    /// In-place version of [`Self::reverse_items`].
    #[inline]
    pub const fn set_reverse_items(&mut self, reversed: bool) {
        self.reverse_items = reversed;
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
        self.set_sort_results(sort);
        self
    }

    /// In-place version of [`Self::sort_results`].
    #[inline]
    pub const fn set_sort_results(&mut self, sort: bool) {
        self.sort_results = sort;
    }

    /// Set the number of threads used by the internal matching engine.
    ///
    /// If `None` (default), use a heuristic choice based on the amount of available
    /// parallelism along with other factors.
    #[must_use]
    #[inline]
    pub const fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.set_threads(threads);
        self
    }

    /// In-place version of [`Self::threads`].
    #[inline]
    pub const fn set_threads(&mut self, threads: Option<NonZero<usize>>) {
        self.threads = threads;
    }

    /// How to perform Unicode normalization (defaults to [`Normalization::Smart`]).
    #[must_use]
    #[inline]
    pub const fn normalization(mut self, normalization: Normalization) -> Self {
        self.set_normalization(normalization);
        self
    }

    /// In-place version of [`Self::normalization`].
    #[inline]
    pub const fn set_normalization(&mut self, normalization: Normalization) {
        self.normalization = normalization.convert();
    }

    /// How to treat case mismatch (defaults to [`CaseMatching::Smart`]).
    #[must_use]
    #[inline]
    pub const fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.set_case_matching(case_matching);
        self
    }

    /// In-place version of [`Self::case_matching`].
    #[inline]
    pub const fn set_case_matching(&mut self, case_matching: CaseMatching) {
        self.case_matching = case_matching.convert();
    }

    /// Enable score bonuses appropriate for matching file paths.
    #[must_use]
    #[inline]
    pub const fn match_paths(mut self) -> Self {
        self.set_match_paths();
        self
    }

    /// In-place version of [`Self::match_paths`].
    #[inline]
    pub const fn set_match_paths(&mut self) {
        self.config.set_match_paths();
    }

    /// Whether to provide a bonus to matches by their distance from the start of the item.
    ///
    /// This is disabled by default and only recommended for autocompletion use-cases, where the expectation is that the user is typing the entire match.
    #[must_use]
    #[inline]
    pub const fn prefer_prefix(mut self, prefer_prefix: bool) -> Self {
        self.set_prefer_prefix(prefer_prefix);
        self
    }

    /// In-place version of [`Self::prefer_prefix`].
    #[inline]
    pub const fn set_prefer_prefix(&mut self, prefer_prefix: bool) {
        self.config.prefer_prefix = prefer_prefix;
    }

    /// Whether or not to highlight matches (defaults to `true`).
    #[must_use]
    #[inline]
    pub const fn highlight(mut self, highlight: bool) -> Self {
        self.set_highlight(highlight);
        self
    }

    /// In-place version of [`Self::highlight`].
    #[inline]
    pub const fn set_highlight(&mut self, highlight: bool) {
        self.match_list_config.highlight = highlight;
    }

    /// Whether to highlight the entire hovered item, and not only the match text
    ///
    /// This can be useful for distinguishing multiline items from each other. The default is false.
    #[must_use]
    #[inline]
    pub const fn highlight_line(mut self, highlight_line: bool) -> Self {
        self.set_highlight_line(highlight_line);
        self
    }

    /// In-place version of [`Self::highlight_line`].
    #[inline]
    pub const fn set_highlight_line(&mut self, highlight_line: bool) {
        self.match_list_config.highlight_line = highlight_line;
    }

    /// How much space to leave when rendering match highlighting (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn highlight_padding(mut self, size: u16) -> Self {
        self.set_highlight_padding(size);
        self
    }

    /// In-place version of [`Self::highlight_padding`].
    #[inline]
    pub const fn set_highlight_padding(&mut self, size: u16) {
        self.match_list_config.highlight_padding = size;
    }

    /// How much space to leave around the hovered item when scrolling (defaults to `3`).
    #[must_use]
    #[inline]
    pub const fn scroll_padding(mut self, size: u16) -> Self {
        self.set_scroll_padding(size);
        self
    }

    /// In-place version of [`Self::scroll_padding`].
    #[inline]
    pub const fn set_scroll_padding(&mut self, size: u16) {
        self.match_list_config.scroll_padding = size;
    }

    /// How much space to leave around the cursor (defaults to `2`).
    #[must_use]
    #[inline]
    pub const fn prompt_padding(mut self, size: u16) -> Self {
        self.set_prompt_padding(size);
        self
    }

    /// In-place version of [`Self::prompt_padding`].
    #[inline]
    pub const fn set_prompt_padding(&mut self, size: u16) {
        self.prompt_config.padding = size;
    }

    /// Set the character used for the prompt indicator.
    ///
    /// The default value is `'>'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn prompt_indicator(mut self, prompt: char) -> Self {
        self.set_prompt_indicator(prompt);
        self
    }

    /// In-place version of [`Self::prompt_indicator`].
    #[inline]
    pub const fn set_prompt_indicator(&mut self, prompt: char) {
        self.chars.prompt = prompt;
    }

    /// Set the character used to mark the hovered item.
    ///
    /// The default value is `'▌'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn hover_indicator(mut self, hover: char) -> Self {
        self.set_hover_indicator(hover);
        self
    }

    /// In-place version of [`Self::hover_indicator`].
    #[inline]
    pub const fn set_hover_indicator(&mut self, hover: char) {
        self.chars.hover = hover;
    }

    /// Set the character used to indicate items queued for selection
    ///
    /// The default value is `'┃'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn queued_indicator(mut self, queued: char) -> Self {
        self.set_queued_indicator(queued);
        self
    }

    /// In-place version of [`Self::queued_indicator`].
    #[inline]
    pub const fn set_queued_indicator(&mut self, queued: char) {
        self.chars.queued = queued;
    }

    /// Set the character used to indicate text that has been truncated.
    ///
    /// The default value is `'…'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn truncation_ellipsis(mut self, ellipsis: char) -> Self {
        self.set_truncation_ellipsis(ellipsis);
        self
    }

    /// In-place version of [`Self::truncation_ellipsis`].
    #[inline]
    pub const fn set_truncation_ellipsis(&mut self, ellipsis: char) {
        self.chars.ellipsis = ellipsis;
    }

    /// Set the character used for the horizontal rule separating the prompt and the match list.
    ///
    /// The default value is `'─'`. The character must have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn separator(mut self, sep: char) -> Self {
        self.set_separator(sep);
        self
    }

    /// In-place version of [`Self::separator`].
    #[inline]
    pub const fn set_separator(&mut self, sep: char) {
        self.chars.separator = sep;
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
        self.set_spinner_chars(spinner);
        self
    }

    /// In-place version of [`Self::spinner_chars`].
    #[inline]
    pub const fn set_spinner_chars(&mut self, spinner: &'static [char]) {
        self.chars.spinner_chars = spinner;
    }

    /// Set the character used to indicate that the matcher is working.
    ///
    /// This indicator is only displayed when the picker is no longer waiting for items; otherwise,
    /// the active spinner takes priority. The default value is `'·'`. The character must
    /// have Unicode width 1.
    #[must_use]
    #[inline]
    pub const fn matching_indicator(mut self, indicator: char) -> Self {
        self.set_matching_indicator(indicator);
        self
    }

    /// In-place version of [`Self::matching_indicator`].
    #[inline]
    pub const fn set_matching_indicator(&mut self, indicator: char) {
        self.chars.matching_indicator = indicator;
    }

    /// Override all previously set picker component characters to a default set.
    ///
    /// If `ascii` is `true` this uses an ASCII-compatible set.
    /// This is equivalent to:
    /// ```
    /// # use nucleo_picker_core::session::ModelOptions;
    /// ModelOptions::new()
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
        self.set_ascii_compatible(ascii);
        self
    }

    /// In-place version of [`Self::ascii_compatible`].
    #[inline]
    pub const fn set_ascii_compatible(&mut self, ascii: bool) {
        self.chars = if ascii {
            PickerChars::ascii()
        } else {
            PickerChars::new()
        };
    }

    /// Provide an initial query string for the prompt (defaults to `""`).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.set_query(query);
        self
    }

    /// In-place version of [`Self::query`].
    #[inline]
    pub fn set_query<Q: Into<String>>(&mut self, query: Q) {
        self.query = query.into();
    }
}
