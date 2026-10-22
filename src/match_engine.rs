//! # Match engine wrapper
//!
//! This module contains a wrapper type around the internal [`Nucleo`](nc::Nucleo) instance.
//! Essentially, this is the match engine and relevant state without any knowledge about selection
//! or rendering.
use std::{
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering, fence},
    },
    thread::available_parallelism,
};

use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};

use crate::{Injector, MatchStatus, Render};

pub(crate) struct MatchEngine<T, R> {
    /// The internal matcher engine.
    nucleo: nc::Nucleo<T>,
    /// Whether the matcher engine should be ticked.
    needs_tick: Arc<AtomicBool>,
    /// A cache for the internal running state.
    nucleo_is_running: bool,
    /// The method which actually renders the items.
    render: Arc<R>,
    /// A cache of the prompt, used to decide if the prompt has changed.
    prompt: String,
    case_matching: NucleoCaseMatching,
    normalization: NucleoNormalization,
    #[cfg(feature = "preview")]
    generation: u32,
    #[cfg(feature = "preview")]
    match_revision: u64,
}

impl<T: Send + Sync + 'static, R> MatchEngine<T, R> {
    pub fn new(
        nucleo_config: nc::Config,
        nucleo_match_config: nc::MatchListConfig,
        threads: Option<NonZero<usize>>,
        render: Arc<R>,
        case_matching: NucleoCaseMatching,
        normalization: NucleoNormalization,
    ) -> Self {
        let needs_tick = Arc::new(AtomicBool::new(false));
        let notify_tick = Arc::clone(&needs_tick);
        let nucleo = nc::Nucleo::with_match_list_config(
            nucleo_config,
            Arc::new(move || notify_tick.store(true, Ordering::Relaxed)),
            // nucleo's API is a bit weird here in that it does not accept `NonZero<usize>`
            threads
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
            nucleo_match_config,
        );
        Self {
            nucleo,
            render,
            case_matching,
            normalization,
            prompt: String::with_capacity(32),
            needs_tick,
            nucleo_is_running: false,
            #[cfg(feature = "preview")]
            generation: 0,
            #[cfg(feature = "preview")]
            match_revision: 0,
        }
    }
    pub fn snapshot(&self) -> &nc::Snapshot<T> {
        self.nucleo.snapshot()
    }

    #[cfg(feature = "preview")]
    pub fn match_revision(&self) -> u64 {
        self.match_revision
    }

    #[cfg(feature = "preview")]
    pub fn item_id(&self, index: u32) -> crate::preview::ItemId {
        crate::preview::ItemId {
            generation: self.generation,
            index,
        }
    }

    #[cfg(feature = "preview")]
    pub fn item_ref(&self, index: u32) -> Option<crate::preview::ItemRef<'_, T>> {
        self.snapshot()
            .get_item(index)
            .map(|item| crate::preview::ItemRef::new(self.item_id(index), item.data))
    }

    pub fn renderer(&self) -> &R {
        &self.render
    }

    /// A convenience function to render a given item using the internal [`Render`] implementation.
    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a>
    where
        R: Render<T>,
    {
        self.render.render(item)
    }

    /// Replace the renderer with a new instance, immediately restarting the matcher engine.
    pub fn reset_renderer(&mut self, render: R) {
        self.restart();
        self.render = render.into();
    }

    /// Get an [`Injector`] to add new match elements.
    pub fn injector(&self) -> Injector<T, R> {
        Injector::new(self.nucleo.injector(), self.render.clone())
    }

    /// Clear all of the items and restart the match engine.
    pub fn restart(&mut self) {
        #[cfg(feature = "preview")]
        {
            self.generation = self
                .generation
                .checked_add(1)
                .expect("picker item generation exhausted");
        }
        self.nucleo.restart(true);
        self.needs_tick.store(true, Ordering::Relaxed);
    }

    /// Replace the prompt string with an updated value.
    pub fn reparse(&mut self, new: &str) {
        let appending = match new.strip_prefix(&self.prompt) {
            Some(rest) => {
                if rest.is_empty() {
                    // the strings are the same so we don't need to do anything
                    return;
                }
                true
            }
            None => false,
        };
        self.nucleo
            .pattern
            .reparse(0, new, self.case_matching, self.normalization, appending);
        self.needs_tick.store(true, Ordering::Relaxed);
        new.clone_into(&mut self.prompt);
    }

    /// Whether or not the list of items is empty.
    pub fn is_empty(&self) -> bool {
        self.nucleo.snapshot().matched_item_count() == 0
    }

    pub fn idx_from_match(&self, n: u32) -> u32 {
        // SAFETY: callers use a selection clamped to this nonempty snapshot, with no intervening tick
        // or restart that could invalidate the rank.
        unsafe {
            self.nucleo
                .snapshot()
                .matches()
                .get_unchecked(n as usize)
                .idx
        }
    }

    /// Finish matching when there is no new input, returning whether a new snapshot was published.
    pub fn finish_matching(&mut self) -> Option<bool> {
        if self.nucleo.active_injectors() != 0 {
            return None;
        }
        // Acquire the final injector releases before reading their items.
        fence(Ordering::Acquire);

        let mut items_changed = false;
        loop {
            self.needs_tick.store(false, Ordering::Relaxed);
            // Wait directly on the workers rather than gating ticks on their notifications.
            let status = self.nucleo.tick(u64::MAX);
            #[cfg(feature = "preview")]
            if status.changed {
                self.match_revision = self.match_revision.wrapping_add(1);
            }
            items_changed |= status.changed;
            self.nucleo_is_running = status.running;
            if !status.running {
                return Some(items_changed);
            }
        }
    }

    /// Check if the internal match workers have returned any new updates for matched items.
    pub fn update(&mut self, millis: u64) -> MatchStatus {
        // Check input closure before consuming notifications. The fence acquires the injectors'
        // Arc drops so that the final push is visible before we can report completion.
        let injecting = self.nucleo.active_injectors() != 0;
        if !injecting {
            fence(Ordering::Acquire);
        }
        let items_changed = if self.needs_tick.swap(false, Ordering::Relaxed) {
            let status = self.nucleo.tick(millis);
            #[cfg(feature = "preview")]
            if status.changed {
                self.match_revision = self.match_revision.wrapping_add(1);
            }

            self.nucleo_is_running = status.running;
            status.changed
        } else {
            false
        };

        MatchStatus {
            items_changed,
            matched_item_count: self.snapshot().matched_item_count(),
            item_count: self.snapshot().item_count(),
            matching: self.nucleo_is_running,
            injecting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_lifetime_updates_status() {
        let mut engine: MatchEngine<&str, _> = MatchEngine::new(
            nc::Config::DEFAULT,
            nc::MatchListConfig {
                sort_results: false,
                reverse_items: false,
            },
            NonZero::new(1),
            crate::render::StrRenderer.into(),
            NucleoCaseMatching::Smart,
            NucleoNormalization::Smart,
        );

        let injector = engine.injector();
        let status = engine.update(0);
        assert!(status.injecting);

        drop(injector);
        let mut status = engine.update(0);
        assert!(!status.injecting);

        while status.matching {
            status = engine.update(5);
        }
    }
}
