use std::{num::NonZero, time::Duration};

use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};

use super::Driver;
use crate::{
    CaseMatching, Normalization, PickerChars,
    match_engine::MatchEngine,
    match_list::{MatchListConfig, MatchListState},
    preview::{BoundaryChars, PreviewConfig},
    prompt::{PromptConfig, PromptState},
};

/// Configuration shared by all picker interfaces.
///
/// Construct a [`Driver`] with [`Self::driver`], or convert into
/// [`PickerOptions`](crate::PickerOptions) to configure built-in interface defaults.
///
// TODO: Move shared option documentation from PickerOptions here before extracting core
pub struct DriverOptions {
    // PickerOptions updates fields in place: const evaluation rejects consuming a nested
    // value with drop glue, even when all of its fields are moved into the result.
    pub(crate) config: nc::Config,
    query: String,
    pub(crate) threads: Option<NonZero<usize>>,
    pub(crate) interval: Duration,
    pub(crate) background_frame_interval: Duration,
    pub(crate) chars: PickerChars,
    pub(crate) match_list_config: MatchListConfig,
    pub(crate) case_matching: NucleoCaseMatching,
    pub(crate) normalization: NucleoNormalization,
    pub(crate) prompt_config: PromptConfig,
    preview_config: PreviewConfig,
    pub(crate) sort_results: bool,
    pub(crate) reverse_items: bool,
}

impl Default for DriverOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DriverOptions {
    /// Initialize with default configuration.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            config: nc::Config::DEFAULT,
            query: String::new(),
            threads: None,
            interval: Duration::from_millis(15),
            background_frame_interval: Duration::from_millis(60),
            chars: PickerChars::new(),
            match_list_config: MatchListConfig::new(),
            case_matching: NucleoCaseMatching::Smart,
            normalization: NucleoNormalization::Smart,
            prompt_config: PromptConfig::new(),
            preview_config: PreviewConfig::new(),
            sort_results: true,
            reverse_items: false,
        }
    }

    /// Construct a driver with this configuration and the provided renderer.
    #[must_use]
    pub fn driver<T: Send + Sync + 'static, R>(self, render: R) -> Driver<T, R> {
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
        prompt.set_query(self.query, crate::util::unicode::grapheme_width);
        engine.reparse(prompt.contents());

        Driver {
            engine,
            list_state,
            prompt,
            preview_config: self.preview_config,
            interval: self.interval,
            background_frame_frequency,
            chars: self.chars,
            reversed,
            restart_notifier: None,
            status_notifier: None,
        }
    }

    /// See [`PickerOptions::reversed`](crate::PickerOptions::reversed).
    #[must_use]
    #[inline]
    pub const fn reversed(mut self, reversed: bool) -> Self {
        self.match_list_config.reversed = reversed;
        self
    }

    /// See [`PickerOptions::reverse_items`](crate::PickerOptions::reverse_items).
    #[must_use]
    #[inline]
    pub const fn reverse_items(mut self, reversed: bool) -> Self {
        self.reverse_items = reversed;
        self
    }

    /// See [`PickerOptions::sort_results`](crate::PickerOptions::sort_results).
    #[must_use]
    #[inline]
    pub const fn sort_results(mut self, sort: bool) -> Self {
        self.sort_results = sort;
        self
    }

    /// See [`PickerOptions::frame_interval`](crate::PickerOptions::frame_interval).
    #[must_use]
    #[inline]
    pub const fn frame_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// See [`PickerOptions::threads`](crate::PickerOptions::threads).
    #[must_use]
    #[inline]
    pub const fn threads(mut self, threads: Option<NonZero<usize>>) -> Self {
        self.threads = threads;
        self
    }

    /// See [`PickerOptions::normalization`](crate::PickerOptions::normalization).
    #[must_use]
    #[inline]
    pub const fn normalization(mut self, normalization: Normalization) -> Self {
        self.normalization = normalization.convert();
        self
    }

    /// See [`PickerOptions::case_matching`](crate::PickerOptions::case_matching).
    #[must_use]
    #[inline]
    pub const fn case_matching(mut self, case_matching: CaseMatching) -> Self {
        self.case_matching = case_matching.convert();
        self
    }

    /// See [`PickerOptions::match_paths`](crate::PickerOptions::match_paths).
    #[must_use]
    #[inline]
    pub const fn match_paths(mut self) -> Self {
        self.config = self.config.match_paths();
        self
    }

    /// See [`PickerOptions::prefer_prefix`](crate::PickerOptions::prefer_prefix).
    #[must_use]
    #[inline]
    pub const fn prefer_prefix(mut self, prefer_prefix: bool) -> Self {
        self.config.prefer_prefix = prefer_prefix;
        self
    }

    /// See [`PickerOptions::highlight`](crate::PickerOptions::highlight).
    #[must_use]
    #[inline]
    pub const fn highlight(mut self, highlight: bool) -> Self {
        self.match_list_config.highlight = highlight;
        self
    }

    /// See [`PickerOptions::highlight_line`](crate::PickerOptions::highlight_line).
    #[must_use]
    #[inline]
    pub const fn highlight_line(mut self, highlight_line: bool) -> Self {
        self.match_list_config.highlight_line = highlight_line;
        self
    }

    /// See [`PickerOptions::highlight_padding`](crate::PickerOptions::highlight_padding).
    #[must_use]
    #[inline]
    pub const fn highlight_padding(mut self, size: u16) -> Self {
        self.match_list_config.highlight_padding = size;
        self
    }

    /// See [`PickerOptions::scroll_padding`](crate::PickerOptions::scroll_padding).
    #[must_use]
    #[inline]
    pub const fn scroll_padding(mut self, size: u16) -> Self {
        self.match_list_config.scroll_padding = size;
        self
    }

    /// See [`PickerOptions::prompt_padding`](crate::PickerOptions::prompt_padding).
    #[must_use]
    #[inline]
    pub const fn prompt_padding(mut self, size: u16) -> Self {
        self.prompt_config.padding = size;
        self
    }

    /// See [`PickerOptions::prompt_indicator`](crate::PickerOptions::prompt_indicator).
    #[must_use]
    #[inline]
    pub const fn prompt_indicator(mut self, prompt: char) -> Self {
        self.chars.prompt = prompt;
        self
    }

    /// See [`PickerOptions::hover_indicator`](crate::PickerOptions::hover_indicator).
    #[must_use]
    #[inline]
    pub const fn hover_indicator(mut self, hover: char) -> Self {
        self.chars.hover = hover;
        self
    }

    /// See [`PickerOptions::queued_indicator`](crate::PickerOptions::queued_indicator).
    #[must_use]
    #[inline]
    pub const fn queued_indicator(mut self, queued: char) -> Self {
        self.chars.queued = queued;
        self
    }

    /// See [`PickerOptions::truncation_ellipsis`](crate::PickerOptions::truncation_ellipsis).
    #[must_use]
    #[inline]
    pub const fn truncation_ellipsis(mut self, ellipsis: char) -> Self {
        self.chars.ellipsis = ellipsis;
        self
    }

    /// See [`PickerOptions::separator`](crate::PickerOptions::separator).
    #[must_use]
    #[inline]
    pub const fn separator(mut self, sep: char) -> Self {
        self.chars.separator = sep;
        self
    }

    /// See [`PickerOptions::spinner_chars`](crate::PickerOptions::spinner_chars).
    #[must_use]
    #[inline]
    pub const fn spinner_chars(mut self, spinner: &'static [char]) -> Self {
        self.chars.spinner_chars = spinner;
        self
    }

    /// See [`PickerOptions::matching_indicator`](crate::PickerOptions::matching_indicator).
    #[must_use]
    #[inline]
    pub const fn matching_indicator(mut self, indicator: char) -> Self {
        self.chars.matching_indicator = indicator;
        self
    }

    /// See [`PickerOptions::ascii_compatible`](crate::PickerOptions::ascii_compatible).
    pub const fn ascii_compatible(mut self, ascii: bool) -> Self {
        self.set_ascii_compatible(ascii);
        self
    }

    pub(crate) const fn set_ascii_compatible(&mut self, ascii: bool) {
        self.chars = if ascii {
            PickerChars::ascii()
        } else {
            PickerChars::new()
        };
        self.preview_config.boundary_chars = if ascii {
            BoundaryChars::ascii()
        } else {
            BoundaryChars::new()
        };
    }

    /// See [`PickerOptions::background_frame_interval`](crate::PickerOptions::background_frame_interval).
    #[must_use]
    #[inline]
    pub const fn background_frame_interval(mut self, interval: Duration) -> Self {
        self.background_frame_interval = interval;
        self
    }

    /// See [`PickerOptions::query`](crate::PickerOptions::query).
    #[must_use]
    #[inline]
    pub fn query<Q: Into<String>>(mut self, query: Q) -> Self {
        self.query = query.into();
        self
    }

    /// See [`PickerOptions::preview_dimensions`](crate::PickerOptions::preview_dimensions).
    #[must_use]
    #[inline]
    pub const fn preview_dimensions(mut self, ratio: f64) -> Self {
        self.set_preview_dimensions(ratio);
        self
    }

    pub(crate) const fn set_preview_dimensions(&mut self, ratio: f64) {
        let valid = ratio >= 0.0 && ratio <= 1.0;
        debug_assert!(valid, "preview ratio must be in 0..=1");
        self.preview_config.ratio = if valid { ratio } else { 0.5 };
    }

    /// See [`PickerOptions::preview_boundary_chars`](crate::PickerOptions::preview_boundary_chars).
    #[must_use]
    #[inline]
    pub const fn preview_boundary_chars(mut self, chars: BoundaryChars) -> Self {
        self.set_preview_boundary_chars(chars);
        self
    }

    pub(crate) const fn set_preview_boundary_chars(&mut self, chars: BoundaryChars) {
        self.preview_config.boundary_chars = chars;
    }
}
