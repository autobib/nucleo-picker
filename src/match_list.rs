//! # The list of match candidates
//!
//! ## Layout rules
//! ### Layout rules for vertical alignment
//!
//! The core layout rules (in decreasing order of priority) are as follows.
//!
//! 1. Respect the padding below and above, except when the cursor is near 0.
//! 2. Render as much of the selection as possible.
//! 3. When the screen size increases, render new elements with lower index, and then elemenets
//!    with higher index.
//! 4. When the screen size decreases, delete whitespace, and then delete elements with higher
//!    index, and then elements with lower index.
//! 5. Change the location of the cursor on the screen as little as possible.
//!
//! ### Layout rules for horizontal alignment of items
//!
//! 1. Multi-line items must have the same amount of scroll for each line.
//! 2. Do not hide highlighted characters.
//! 3. Prefer to make the scroll as small as possible.
//!
//! ## Module organization
//! This module contains the core [`MatchList`] struct. See the various update methods:
//!
//! 1. [`MatchList::resize`] for screen size changes.
//! 2. [`MatchList::update_items`] for item list changes.
//! 3. [`MatchList::selection_incr`] if the selection increases.
//! 4. [`MatchList::selection_decr`] if the selection decreases.
//! 5. [`MatchList::reset`] to set the cursor to 0 and completely redraw the screen.
//! 6. [`MatchList::reparse`] to change the prompt string.
//! 7. [`MatchList::update`] to wait for any changes in the match engine.
//!
//! Instead of merging the various methods together, we individually maintain methods for the
//! various changes for performance so that only the layout computations required for the relevant
//! changes are performed. For instance, on most frame renders, item updates are the most common.
//!
//! The actual implementations of the various layout methods are contained in the relevant
//! sub-modules.
#[cfg(test)]
mod tests;

mod draw;
mod item;
mod layout;
mod span;
mod unicode;

use std::{ops::Range, sync::Arc};

use self::{
    layout::{reset, resize, selection, update},
    unicode::Span,
};
use crate::{Injector, Render, incremental::Incremental};

use nucleo::{
    self as nc,
    pattern::{CaseMatching as NucleoCaseMatching, Normalization as NucleoNormalization},
};

/// An event that modifies the selection in the match list.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchListEvent {
    /// Move the selection up `usize` items.
    Up(usize),
    /// Move the selection down `usize` items.
    Down(usize),
    /// Reset the selection to the start of the match list.
    Reset,
}

/// A trait to describe items with a certain size.
pub trait ItemSize {
    /// The size of the item on the screen.
    fn size(&self) -> usize;
}

/// A list of items with variable sizes.
pub trait ItemList {
    /// The item type of list.
    type Item<'a>: ItemSize
    where
        Self: 'a;

    /// The total number items in the list.
    fn total(&self) -> u32;

    /// An iterator over items below the cursor, iterating downwards.
    fn lower(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items below and including the cursor, iterating downwards.
    fn lower_inclusive(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above cursor, iterating upwards.
    fn higher(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above and including the cursor, iterating upwards.
    fn higher_inclusive(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;
}

/// An automatic extension trait for an [`ItemList`].
trait ItemListExt: ItemList {
    /// Wrap the item sizes returned by [`lower`](ItemList::lower)
    /// into a [`Incremental`].
    fn sizes_lower<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`lower_inclusive`](ItemList::lower_inclusive)
    /// into a [`Incremental`].
    fn sizes_lower_inclusive<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower_inclusive(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`higher`](ItemList::higher)
    /// into an [`Incremental`].
    fn sizes_higher<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`higher_inclusive`](ItemList::higher)
    /// into an [`Incremental`].
    fn sizes_higher_inclusive<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher_inclusive(cursor).map(|item| item.size()))
    }
}

impl<B: ItemList> ItemListExt for B {}

/// Context from the previous render used to update the screen correctly.
#[derive(Debug)]
struct MatchListState {
    selection: u32,
    below: u16,
    above: u16,
    size: u16,
}

/// Configuration used internally in the [`PickerState`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MatchListConfig {
    /// Whether or not to do match highlighting.
    pub highlight: bool,
    /// Whether or not the screen is reversed.
    pub reversed: bool,
    /// The amount of padding around highlighted matches.
    pub highlight_padding: u16,
    /// The amount of padding when scrolling.
    pub scroll_padding: u16,
    /// Case matching behaviour for matches.
    pub case_matching: NucleoCaseMatching,
    /// Normalization behaviour for matches.
    pub normalization: NucleoNormalization,
}

impl MatchListConfig {
    pub const fn new() -> Self {
        Self {
            highlight: true,
            reversed: false,
            highlight_padding: 3,
            scroll_padding: 3,
            case_matching: NucleoCaseMatching::Smart,
            normalization: NucleoNormalization::Smart,
        }
    }
}

impl Default for MatchListConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A buffer to hold match highlight information and cached line information in an underlying
/// string slice.
pub struct IndexBuffer {
    /// Spans used to render items.
    spans: Vec<Span>,
    /// Sub-slices of `spans` corresponding to lines.
    lines: Vec<Range<usize>>,
    /// Indices generated from a match.
    indices: Vec<u32>,
}

impl IndexBuffer {
    /// Create a new buffer.
    pub fn new() -> Self {
        Self {
            spans: Vec::with_capacity(16),
            lines: Vec::with_capacity(4),
            indices: Vec::with_capacity(16),
        }
    }
}

/// A component for representing the list of successful matches.
///
/// This component has two main parts: the internal [`nucleo::Nucleo`] match engine, as well as a
/// stateful representation of the match items which are currently on the screen. See the module
/// level documentation for more detail.
pub struct MatchList<T: Send + Sync + 'static, R> {
    /// The current selection; this corresponds to a valid index if and only if the current
    /// snapshot has more than one element.
    selection: u32,
    /// The size of the screen last time the screen changed.
    size: u16,
    /// The layout buffer below and including the matched item.
    below: Vec<usize>,
    /// The layout buffer above the matched item.
    above: Vec<usize>,
    /// Configuration for drawing.
    config: MatchListConfig,
    /// The internal matcher engine.
    nucleo: nc::Nucleo<T>,
    /// Scratch space for index computations during rendering.
    scratch: IndexBuffer,
    /// The method which actually renders the items.
    render: Arc<R>,
    /// The internal matcher.
    matcher: nc::Matcher,
    /// A cache of the prompt, used to decide if the prompt has changed.
    prompt: String,
}

impl<T: Send + Sync + 'static, R> MatchList<T, R> {
    /// Initialize a new [`MatchList`] with the provided configuration and initial state.
    pub fn new(
        config: MatchListConfig,
        nucleo_config: nc::Config,
        nucleo: nc::Nucleo<T>,
        render: Arc<R>,
    ) -> Self {
        Self {
            size: 0,
            selection: 0,
            below: Vec::with_capacity(128),
            above: Vec::with_capacity(128),
            config,
            nucleo,
            matcher: nc::Matcher::new(nucleo_config),
            render,
            scratch: IndexBuffer::new(),
            prompt: String::with_capacity(32),
        }
    }

    pub fn reversed(&self) -> bool {
        self.config.reversed
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
        self.update_items();
    }

    /// Replace the internal [`nucleo`] configuration.
    pub fn update_nucleo_config(&mut self, config: nc::Config) {
        self.nucleo.update_config(config);
    }

    /// Returns a self-contained representation of the screen state required for correct layout
    /// update computations.
    fn state(&self) -> MatchListState {
        let below = self.below.iter().sum::<usize>() as u16;
        let above = self.above.iter().sum::<usize>() as u16;
        MatchListState {
            selection: self.selection,
            below: self.size - above,
            above: self.size - below,
            size: self.size,
        }
    }

    /// The total amount of whitespace present in the displayed match list.
    fn whitespace(&self) -> u16 {
        self.size
            - self.below.iter().sum::<usize>() as u16
            - self.above.iter().sum::<usize>() as u16
    }

    /// The amount of padding corresponding to the provided size.
    pub fn padding(&self, size: u16) -> u16 {
        self.config.scroll_padding.min(size.saturating_sub(1) / 2)
    }

    /// Replace the prompt string with an updated value.
    pub fn reparse(&mut self, new: &str) {
        // appending if the new value has the previous value as a prefix and also does not end in a
        // trailing unescaped '\\'
        let appending = match new.strip_prefix(&self.prompt) {
            Some(rest) => {
                if rest.is_empty() {
                    // the strings are the same so we don't need to do anything
                    return;
                } else {
                    true
                }
            }
            None => false,
        };
        self.nucleo.pattern.reparse(
            0,
            new,
            self.config.case_matching,
            self.config.normalization,
            appending,
        );
        self.prompt = new.to_owned();
    }

    /// Whether or not the list of items is empty.
    pub fn is_empty(&self) -> bool {
        self.nucleo.snapshot().matched_item_count() == 0
    }

    pub fn selection(&self) -> u32 {
        self.selection
    }

    pub fn max_selection(&self) -> u32 {
        self.nucleo
            .snapshot()
            .matched_item_count()
            .saturating_sub(1)
    }

    pub fn get_item(&self, n: u32) -> Option<nc::Item<'_, T>> {
        self.nucleo.snapshot().get_matched_item(n)
    }

    /// Return the range corresponding to the matched items visible on the screen.
    pub fn selection_range(&self) -> std::ops::RangeInclusive<u32> {
        if self.config.reversed {
            self.selection - self.above.len() as u32..=self.selection + self.below.len() as u32 - 1
        } else {
            self.selection + 1 - self.below.len() as u32..=self.selection + self.above.len() as u32
        }
    }

    /// Recompute the match layout when the screen size has changed.
    pub fn resize(&mut self, total_size: u16) {
        // check for zero, so the 'clamp' call dows not fail
        if total_size == 0 {
            self.size = 0;
            self.above.clear();
            self.below.clear();
            return;
        }

        let buffer = self.nucleo.snapshot();

        // check for no elements, so the `sizes_below` and `sizes_above` calls do not fail
        if buffer.total() == 0 {
            self.size = total_size;
            return;
        }

        let padding = self.padding(total_size);

        let mut previous = self.state();

        if self.config.reversed {
            // since the padding could change, make sure the value of 'below' is valid for the new
            // padding values
            previous.below = previous.below.clamp(padding, total_size - padding - 1);

            let sizes_below_incl = buffer.sizes_higher_inclusive(self.selection, &mut self.below);
            let sizes_above = buffer.sizes_lower(self.selection, &mut self.above);

            if self.size <= total_size {
                resize::larger_rev(previous, total_size, padding, sizes_below_incl, sizes_above);
            } else {
                resize::smaller_rev(
                    previous,
                    total_size,
                    padding,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            }
        } else {
            // since the padding could change, make sure the value of 'above' is valid for the new
            // padding values
            previous.above = previous.above.clamp(padding, total_size - padding - 1);

            let sizes_below_incl = buffer.sizes_lower_inclusive(self.selection, &mut self.below);
            let sizes_above = buffer.sizes_higher(self.selection, &mut self.above);

            if self.size <= total_size {
                resize::larger(previous, total_size, sizes_below_incl, sizes_above);
            } else {
                resize::smaller(previous, total_size, padding, sizes_below_incl, sizes_above);
            }
        }

        self.size = total_size;
    }

    /// Check if the internal match workers have returned any new updates for matched items.
    pub fn update(&mut self, millis: u64) -> bool {
        let status = self.nucleo.tick(millis);
        if status.changed {
            self.update_items();
        }
        status.changed
    }

    /// Reset the layout, setting the cursor to '0' and rendering the items.
    pub fn reset(&mut self) -> bool {
        let buffer = self.nucleo.snapshot();
        let padding = self.padding(self.size);
        if self.selection != 0 {
            if self.config.reversed {
                let sizes_below_incl = buffer.sizes_higher_inclusive(0, &mut self.below);
                self.above.clear();

                reset::reset_rev(self.size, sizes_below_incl);
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(0, &mut self.below);
                let sizes_above = buffer.sizes_higher(0, &mut self.above);

                reset::reset(self.size, padding, sizes_below_incl, sizes_above);
            }

            self.selection = 0;
            true
        } else {
            false
        }
    }

    /// Update the layout with the modified item list.
    pub fn update_items(&mut self) {
        let buffer = self.nucleo.snapshot();
        // clamp the previous cursor in case it has become invalid for the updated items
        self.selection = self.selection.min(buffer.total().saturating_sub(1));
        let previous = self.state();
        let padding = self.padding(self.size);

        if buffer.total() > 0 {
            if self.config.reversed {
                let sizes_below_incl =
                    buffer.sizes_higher_inclusive(self.selection, &mut self.below);
                let sizes_above = buffer.sizes_lower(self.selection, &mut self.above);

                update::items_rev(previous, padding, sizes_below_incl, sizes_above);
            } else {
                let sizes_below_incl =
                    buffer.sizes_lower_inclusive(self.selection, &mut self.below);
                let sizes_above = buffer.sizes_higher(self.selection, &mut self.above);

                update::items(previous, padding, sizes_below_incl, sizes_above);
            }
        } else {
            self.below.clear();
            self.above.clear();
            self.selection = 0;
        }
    }

    #[inline]
    pub fn set_selection(&mut self, new_selection: u32) -> bool {
        let buffer = self.nucleo.snapshot();
        let new_selection = new_selection.min(buffer.total().saturating_sub(1));

        let previous = self.state();
        let padding = self.padding(self.size);

        if new_selection == 0 {
            self.reset()
        } else if new_selection > self.selection {
            if self.config.reversed {
                let sizes_below_incl =
                    buffer.sizes_higher_inclusive(new_selection, &mut self.below);
                let sizes_above = buffer.sizes_lower(new_selection, &mut self.above);

                selection::incr_rev(
                    previous,
                    new_selection,
                    padding,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(new_selection, &mut self.below);
                let sizes_above = buffer.sizes_higher(new_selection, &mut self.above);

                selection::incr(
                    previous,
                    new_selection,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            }

            self.selection = new_selection;

            true
        } else if new_selection < self.selection {
            if self.config.reversed {
                let sizes_below_incl =
                    buffer.sizes_higher_inclusive(new_selection, &mut self.below);
                let sizes_above = buffer.sizes_lower(new_selection, &mut self.above);

                selection::decr_rev(
                    previous,
                    new_selection,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(new_selection, &mut self.below);
                let sizes_above = buffer.sizes_higher(new_selection, &mut self.above);

                selection::decr(
                    previous,
                    new_selection,
                    padding,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            }

            self.selection = new_selection;

            true
        } else {
            false
        }
    }

    /// Increment the selection by the given amount.
    pub fn selection_incr(&mut self, increase: u32) -> bool {
        let new_selection = self
            .selection
            .saturating_add(increase)
            .min(self.nucleo.snapshot().total().saturating_sub(1));

        self.set_selection(new_selection)
    }

    /// Decrement the selection by the given amount.
    pub fn selection_decr(&mut self, decrease: u32) -> bool {
        let new_selection = self.selection.saturating_sub(decrease);

        self.set_selection(new_selection)
    }
}
