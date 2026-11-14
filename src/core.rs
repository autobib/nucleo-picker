use std::{num::NonZero, time::Duration};

#[cfg(feature = "preview")]
use crate::preview::PreviewConfig;
use crate::{
    Injector, Multi, Picker, PickerChars, PickerRef, event::PickerStatus,
    match_engine::MatchEngine, match_list::MatchListState, observer::Notifier, prompt::PromptState,
};

pub struct PickerCore<T, R> {
    pub(crate) engine: MatchEngine<T, R>,
    pub(crate) list_state: MatchListState,
    pub(crate) chars: PickerChars,
    pub(crate) multi_defaults: Multi,
    pub(crate) prompt: PromptState,
    #[cfg(feature = "preview")]
    pub(crate) preview_config: PreviewConfig,
    #[cfg(feature = "preview")]
    pub(crate) preview_cache_size: Option<NonZero<usize>>,
    pub(crate) interval: Duration,
    pub(crate) background_frame_frequency: NonZero<usize>,
    pub(crate) reversed: bool,
    pub(crate) restart_notifier: Option<Notifier<Injector<T, R>>>,
    pub(crate) status_notifier: Option<Notifier<PickerStatus>>,
}

impl<T, R> PickerCore<T, R> {
    #[inline]
    pub(crate) fn with_interface<I>(self, interface: I) -> Picker<T, R, I> {
        Picker {
            core: self,
            interface,
        }
    }

    #[inline]
    pub(crate) fn as_interface<I>(&mut self, interface: I) -> PickerRef<'_, T, R, I> {
        PickerRef {
            core: self,
            interface,
        }
    }
}
