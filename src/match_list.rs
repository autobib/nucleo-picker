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

use std::{
    collections::{BTreeMap, btree_map::Entry},
    num::NonZero,
    ops::Range,
    sync::Arc,
};

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
///
/// Note that the events
/// [`ToggleUp`](MatchListEvent::ToggleUp),
/// [`ToggleDown`](MatchListEvent::ToggleDown), and
/// [`DeselectAll`](MatchListEvent::DeselectAll) are only handled by the picker in [multiple
/// selection mode](crate::Picker#multiple-selections).
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchListEvent {
    /// Move the selection up `usize` items.
    Up(usize),
    /// Toggle the selection then move up `usize` items.
    ToggleUp(usize),
    /// Move the selection down `usize` items.
    Down(usize),
    /// Toggle the selection then move down `usize` items.
    ToggleDown(usize),
    /// Deselect all queued selections.
    DeselectAll,
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

pub trait Queued {
    type Output<'a, T: Send + Sync + 'static>;

    fn is_empty(&self) -> bool;

    fn clear(&mut self) -> bool;

    fn toggle(&mut self, idx: u32) -> bool;

    fn is_queued(&self, idx: u32) -> bool;

    fn count(&self, limit: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)>;

    fn init(limit: Option<NonZero<u32>>) -> Self;

    fn into_only_selection<'a, T: Send + Sync + 'static>(
        self,
        snapshot: &'a nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'a, T>;

    fn into_selection<'a, T: Send + Sync + 'static>(
        self,
        snapshot: &'a nucleo::Snapshot<T>,
    ) -> Self::Output<'a, T>;
}

impl Queued for () {
    type Output<'a, T: Send + Sync + 'static> = Option<&'a T>;

    #[inline]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline]
    fn clear(&mut self) -> bool {
        false
    }

    #[inline]
    fn toggle(&mut self, _: u32) -> bool {
        false
    }

    #[inline]
    fn is_queued(&self, _: u32) -> bool {
        false
    }

    #[inline]
    fn init(_: Option<NonZero<u32>>) -> Self {}

    #[inline]
    fn into_selection<'a, T: Send + Sync + 'static>(
        self,
        _: &'a nucleo::Snapshot<T>,
    ) -> Self::Output<'a, T> {
        None
    }

    #[inline]
    fn into_only_selection<'a, T: Send + Sync + 'static>(
        self,
        snapshot: &'a nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'a, T> {
        Some(snapshot.get_item(idx).unwrap().data)
    }

    #[inline]
    fn count(&self, _: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)> {
        None
    }
}

impl Queued for SelectedIndices {
    type Output<'a, T: Send + Sync + 'static> = Selection<'a, T>;

    #[inline]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    fn clear(&mut self) -> bool {
        if self.is_empty() {
            false
        } else {
            self.inner.clear();
            true
        }
    }

    #[inline]
    fn toggle(&mut self, idx: u32) -> bool {
        let n = self.inner.len() as u32;
        match self.inner.entry(idx) {
            Entry::Occupied(occupied_entry) => {
                occupied_entry.remove_entry();
                true
            }
            Entry::Vacant(vacant_entry) => {
                if self.limit.is_none_or(|l| n < l.get()) {
                    vacant_entry.insert(());
                    true
                } else {
                    false
                }
            }
        }
    }

    #[inline]
    fn is_queued(&self, idx: u32) -> bool {
        self.inner.contains_key(&idx)
    }

    #[inline]
    fn init(limit: Option<NonZero<u32>>) -> Self {
        Self {
            inner: BTreeMap::new(),
            limit,
        }
    }

    #[inline]
    fn into_selection<'a, T: Send + Sync + 'static>(
        self,
        snapshot: &'a nucleo::Snapshot<T>,
    ) -> Self::Output<'a, T> {
        Self::Output {
            snapshot,
            queued: self,
        }
    }

    #[inline]
    fn into_only_selection<'a, T: Send + Sync + 'static>(
        mut self,
        snapshot: &'a nucleo::Snapshot<T>,
        idx: u32,
    ) -> Self::Output<'a, T> {
        self.inner.insert(idx, ());
        Self::Output {
            snapshot,
            queued: self,
        }
    }

    #[inline]
    fn count(&self, limit: Option<NonZero<u32>>) -> Option<(u32, Option<NonZero<u32>>)> {
        Some((self.inner.len() as u32, limit))
    }
}

pub struct SelectedIndices {
    // FIXME: replace with BTreeSet when the entry API lands
    // > https://github.com/rust-lang/rust/issues/133549)
    inner: BTreeMap<u32, ()>,
    limit: Option<NonZero<u32>>,
}

/// The selected items when the picker quits.
///
/// This is the return type of the various `pick_multi*` methods of a [`Picker`](crate::Picker).
/// Iterate over the picked items with [`iter`](Self::iter). If no items were selected, ththe struct
/// will be [empty](Self::is_empty). Also see the docs on
/// [multiple selections](crate::Picker#multiple-selections)
///
/// The lifetime of this struct is bound to the lifetime of the picker from which it originated.
pub struct Selection<'a, T: Send + Sync + 'static> {
    snapshot: &'a nc::Snapshot<T>,
    // FIXME: replace with BTreeSet when the entry API lands
    // > https://github.com/rust-lang/rust/issues/133549)
    queued: SelectedIndices,
}

impl<'a, T: Send + Sync + 'static> Selection<'a, T> {
    /// Returns an iterator over the other selected items.
    ///
    ///
    /// The iterator contains each selected item exactly once, sorted by index based on the order
    /// in which the picker received the items. Note that items are deduplicated based on the
    /// selection index instead of using any properties of the type `T` itself.
    ///
    /// The iterator will be empty if the picker quit without selecting any items.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a T> + DoubleEndedIterator {
        self.queued.inner.keys().map(|idx| {
            // SAFETY: the indices were produced by the same snapshot which is stored inside this
            // struct, and the lifetime prevents the indices from being invalidated until this struct
            // is dropped
            unsafe { self.snapshot.get_item_unchecked(*idx).data }
        })
    }

    /// Returns if there were no selected items.
    pub fn is_empty(&self) -> bool {
        self.queued.inner.is_empty()
    }

    /// Returns the number of selected items.
    pub fn len(&self) -> usize {
        self.queued.inner.len()
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
            // queued_items: HashMap::with_hasher(BuildHasherDefault::new()),
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

    fn idx_from_match_unchecked(&self, n: u32) -> u32 {
        self.nucleo
            .snapshot()
            .matches()
            .get(n as usize)
            .unwrap()
            .idx
    }

    pub fn toggle_queued_item<Q: Queued>(&mut self, queued_items: &mut Q, n: u32) -> bool {
        queued_items.toggle(self.idx_from_match_unchecked(n))
    }

    pub fn select_none<Q: Queued>(&self, mut queued_items: Q) -> Q::Output<'_, T> {
        queued_items.clear();
        self.select_queued(queued_items)
    }

    pub fn select_one<Q: Queued>(&self, queued_items: Q, n: u32) -> Q::Output<'_, T> {
        let idx = self.idx_from_match_unchecked(n);
        let snapshot = self.nucleo.snapshot();
        queued_items.into_only_selection(snapshot, idx)
    }

    pub fn select_queued<Q: Queued>(&self, queued_items: Q) -> Q::Output<'_, T> {
        let snapshot = self.nucleo.snapshot();
        queued_items.into_selection(snapshot)
    }

    /// Return the range corresponding to the matched items visible on the screen.
    pub fn selection_range(&self) -> std::ops::RangeInclusive<usize> {
        if self.config.reversed {
            self.selection as usize - self.above.len()
                ..=self.selection as usize + self.below.len() - 1
        } else {
            self.selection as usize + 1 - self.below.len()
                ..=self.selection as usize + self.above.len()
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
    #[cfg(test)]
    pub fn selection_incr(&mut self, increase: u32) -> bool {
        let new_selection = self
            .selection
            .saturating_add(increase)
            .min(self.nucleo.snapshot().total().saturating_sub(1));

        self.set_selection(new_selection)
    }

    /// Decrement the selection by the given amount.
    #[cfg(test)]
    pub fn selection_decr(&mut self, decrease: u32) -> bool {
        let new_selection = self.selection.saturating_sub(decrease);

        self.set_selection(new_selection)
    }
}
