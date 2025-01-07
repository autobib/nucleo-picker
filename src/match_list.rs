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
use crate::{incremental::Incremental, Injector, Render};

use nucleo::{
    self as nc,
    pattern::{CaseMatching, Normalization},
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
    #[allow(unused)]
    fn lower(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items below and including the cursor, iterating downwards.
    fn lower_inclusive(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above cursor, iterating upwards.
    fn higher(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above and including the cursor, iterating upwards.
    #[allow(unused)]
    fn higher_inclusive(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;
}

/// An automatic extension trait for an [`ItemList`].
trait ItemListExt: ItemList {
    /// Wrap the item sizes returned by [`below`](ItemList::lower)
    /// into a [`Incremental`].
    fn sizes_lower_inclusive<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower_inclusive(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`above`](ItemList::higher)
    /// into an [`Incremental`].
    fn sizes_higher<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher(cursor).map(|item| item.size()))
    }
}

impl<B: ItemList> ItemListExt for B {}

/// Context from the previous render used to update the screen correctly.
struct Previous {
    selection: u32,
    above: u16,
    size: u16,
}

/// Configuration used internally in the [`PickerState`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MatchListConfig {
    /// Whether or not to do match highlighting.
    pub highlight: bool,
    /// If the screen is reversed.
    pub reversed: bool,
    /// The amount of padding for the highlights.
    pub highlight_padding: u16,
    /// The amount of padding when scrolling.
    pub scroll_padding: u16,
    /// Case matching behaviour for matches.
    pub case_matching: CaseMatching,
    /// Normalization behaviour for matches.
    pub normalization: Normalization,
}

impl Default for MatchListConfig {
    fn default() -> Self {
        Self {
            highlight: true,
            reversed: false,
            highlight_padding: 3,
            scroll_padding: 3,
            case_matching: CaseMatching::default(),
            normalization: Normalization::default(),
        }
    }
}
pub struct IndexBuffer {
    /// Spans used to render items.
    spans: Vec<Span>,
    /// Sub-slices of `spans` corresponding to lines.
    lines: Vec<Range<usize>>,
    /// Indices generated from a match.
    indices: Vec<u32>,
}

impl IndexBuffer {
    pub fn new() -> Self {
        Self {
            spans: Vec::with_capacity(16),
            lines: Vec::with_capacity(4),
            indices: Vec::with_capacity(16),
        }
    }
}

/// Stateful representation of the screen layout.
pub struct MatchList<T: Send + Sync + 'static, R> {
    selection: u32,
    // above: u16,
    size: u16,
    /// Whether or not the layout is 'reversed'.
    reversed: bool,
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

impl<T: Send + Sync + 'static, R: Render<T>> MatchList<T, R> {
    pub fn new(
        config: MatchListConfig,
        matcher_config: nc::Config,
        nucleo: nc::Nucleo<T>,
        render: Arc<R>,
    ) -> Self {
        Self {
            size: 0,
            selection: 0,
            below: Vec::with_capacity(128),
            above: Vec::with_capacity(128),
            reversed: false,
            config,
            nucleo,
            matcher: nc::Matcher::new(matcher_config),
            render,
            scratch: IndexBuffer::new(),
            prompt: String::with_capacity(32),
        }
    }

    pub fn render<'a>(&self, item: &'a T) -> <R as Render<T>>::Str<'a> {
        self.render.render(item)
    }

    pub fn reset_renderer(&mut self, render: R) {
        self.restart();
        self.render = render.into();
    }

    pub fn injector(&self) -> Injector<T, R> {
        Injector::new(self.nucleo.injector(), self.render.clone())
    }

    pub fn restart(&mut self) {
        self.nucleo.restart(true);
        self.reset();
    }

    pub fn update_nucleo_config(&mut self, config: nc::Config) {
        self.nucleo.update_config(config);
    }

    fn state(&self) -> Previous {
        Previous {
            selection: self.selection,
            above: self.size - self.below.iter().sum::<usize>() as u16,
            size: self.size,
        }
    }

    fn whitespace(&self) -> u16 {
        self.size
            - self.below.iter().sum::<usize>() as u16
            - self.above.iter().sum::<usize>() as u16
    }

    pub fn padding(&self, size: u16) -> u16 {
        self.config.scroll_padding.min(size.saturating_sub(1) / 2)
    }

    pub fn reparse(&mut self, new: &str) {
        // appending if the new value has the previous value as a prefix and also does not end in a
        // trailing unescaped '\\'
        let appending = match new.strip_prefix(&self.prompt) {
            Some(rest) => {
                if rest.is_empty() {
                    // the strings are the same so we don't need to do anything
                    return;
                } else {
                    // TODO: fixed in nucleo 0.5.1; remove when updated
                    (self
                        .prompt
                        .bytes()
                        .rev()
                        .take_while(|ch| *ch == b'\\')
                        .count()
                        % 2)
                        == 0
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

    pub fn is_empty(&self) -> bool {
        self.nucleo.snapshot().matched_item_count() == 0
    }

    pub fn selected_item(&self) -> Option<nc::Item<'_, T>> {
        self.nucleo.snapshot().get_matched_item(self.selection)
    }

    pub fn selection_range(&self) -> std::ops::RangeInclusive<u32> {
        self.selection + 1 - self.below.len() as u32..=self.selection + self.above.len() as u32
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

        // since the padding could change, make sure the value of 'above' is valid for the new
        // padding values
        previous.above = previous.above.clamp(padding, total_size - padding - 1);

        let sizes_below_incl = buffer.sizes_lower_inclusive(self.selection, &mut self.below);
        let sizes_above = buffer.sizes_higher(self.selection, &mut self.above);

        if self.reversed {
            if self.size <= total_size {
                todo!();
            } else {
                todo!();
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if self.size <= total_size {
                resize::larger(previous, total_size, sizes_below_incl, sizes_above);
            } else {
                resize::smaller(previous, total_size, padding, sizes_below_incl, sizes_above);
            }
        }

        self.size = total_size;
    }

    pub fn update(&mut self) -> bool {
        let status = self.nucleo.tick(10);
        if status.changed {
            self.update_items();
        }
        status.changed
    }

    /// Reset the layout, setting the cursor to '0' and rendering the items.
    fn reset(&mut self) -> bool {
        let buffer = self.nucleo.snapshot();
        let padding = self.padding(self.size);
        if self.selection != 0 {
            let sizes_below_incl = buffer.sizes_lower_inclusive(0, &mut self.below);
            if self.reversed {
                reset::reset_rev(self.size, sizes_below_incl);
            } else {
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
            let sizes_below_incl = buffer.sizes_lower_inclusive(self.selection, &mut self.below);
            let sizes_above = buffer.sizes_higher(self.selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
                update::items(previous, padding, sizes_below_incl, sizes_above);
            }
        } else {
            self.below.clear();
            self.above.clear();
            self.selection = 0;
        }
    }

    /// Increment the selection by the given amount.
    pub fn selection_incr(&mut self, increase: u32) -> bool {
        let buffer = self.nucleo.snapshot();

        let new_selection = self
            .selection
            .saturating_add(increase)
            .min(buffer.total().saturating_sub(1));

        let previous = self.state();
        let padding = self.padding(self.size);

        if new_selection != self.selection {
            let sizes_below_incl = buffer.sizes_lower_inclusive(new_selection, &mut self.below);
            let sizes_above = buffer.sizes_higher(new_selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
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
        } else {
            false
        }
    }

    /// Decrement the selection by the given amount.
    pub fn selection_decr(&mut self, decrease: u32) -> bool {
        let buffer = self.nucleo.snapshot();
        let new_selection = self.selection.saturating_sub(decrease);

        let previous = self.state();
        let padding = self.padding(self.size);

        if new_selection != self.selection {
            let sizes_below_incl = buffer.sizes_lower_inclusive(new_selection, &mut self.below);
            let sizes_above = buffer.sizes_higher(new_selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
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
}
