//! # Match engine wrapper
//!
//! This module contains a wrapper type around the internal [`Nucleo`](nc::Nucleo) instance.
//! Essentially, this is the match engine and relevant state without any knowledge about selection
//! or rendering.
use std::{
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::available_parallelism,
};

use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};

use crate::{Injector, Render};

#[derive(Clone, Copy)]
pub(crate) struct MatchStatus {
    pub items_changed: bool,
    pub matched: u32,
    pub total: u32,
    pub matching: bool,
    pub injecting: bool,
}

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
        }
    }
    pub fn snapshot(&self) -> &nc::Snapshot<T> {
        self.nucleo.snapshot()
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
        self.nucleo.restart(true);
        self.needs_tick.store(true, Ordering::Relaxed);
    }

    /// Replace the internal [`nucleo`] configuration.
    pub fn update_nucleo_config(&mut self, config: nc::Config) {
        self.nucleo.update_config(config);
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

    /// Check if the internal match workers have returned any new updates for matched items.
    pub fn update(&mut self, millis: u64) -> MatchStatus {
        let items_changed = if self.needs_tick.swap(false, Ordering::Relaxed) {
            let status = self.nucleo.tick(millis);

            self.nucleo_is_running = status.running;
            status.changed
        } else {
            false
        };

        MatchStatus {
            items_changed,
            matched: self.snapshot().matched_item_count(),
            total: self.snapshot().item_count(),
            matching: self.nucleo_is_running,
            injecting: self.nucleo.active_injectors() != 0,
        }
    }
}
