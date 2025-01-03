//! # Computing the layout for matches
//!
//! ## Layout rules
//!
//! The core layout rules (in decreasing order of priority) are as follows.
//!
//! 1. Respect the padding below and above, except when the cursor is near 0.
//! 2. Render as much of the selection as possible.
//! 3. When the screen size increases, render new elements with lower index, and then elemenets
//!    with higher index.
//! 4. When the screen size decreases, delete whitespace, and then delete elements with higher
//!    index, and then elements with lower index.
//!
//!
//! ## Module organization
//!
//! This module contains the core [`Matcher`] struct, which gives an internal representation of
//! the item layout with the [`Matcher::view`] method, and also supports various types of updates:
//!
//! 1. [`Matcher::resize`] for screen size changes.
//! 2. [`Matcher::update_items`] for item list changes.
//! 3. [`Matcher::selection_incr`] if the selection increases.
//! 4. [`Matcher::selection_decr`] if the selection decreases.
//! 5. [`Matcher::reset`] to set the cursor to 0 and completely redraw the screen.
//!
//!
//! Instead of merging the various methods together, we individually maintain methods for the
//! various changes for performance so that only the layout computations required for the relevant
//! changes are performed. For instance, on most frame renders, item updates are the most common.
//!
//! The actual implementations of the various layout methods are contained in the relevant
//! sub-modules.
#[cfg(test)]
mod tests;

mod reset;
mod resize;
mod selection;
mod update;

use crate::incremental::Incremental;

/// A trait to describe items with a certain size.
pub trait ItemSize {
    /// The size of the item on the screen.
    fn size(&self) -> usize;
}

/// A buffer of items with variable sizes.
pub trait VariableSizeBuffer {
    /// The item type of the buffer.
    type Item<'a>: ItemSize
    where
        Self: 'a;

    /// The total number items contained in the buffer.
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

/// An automatic extension trait for a [`VariableSizeBuffer`].
trait VariableSizeBufferExt: VariableSizeBuffer {
    /// Wrap the item sizes returned by [`below`](VariableSizeBuffer::below)
    /// into a [`Incremental`].
    fn sizes_below<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower_inclusive(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`above`](VariableSizeBuffer::above)
    /// into an [`Incremental`].
    fn sizes_above<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher(cursor).map(|item| item.size()))
    }
}

impl<B: VariableSizeBuffer> VariableSizeBufferExt for B {}

/// A view into a [`Matcher`] at a given point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutView<'a> {
    /// The number of lines to render for each item beginning below the screen index and rendering
    /// downwards.
    pub below: &'a [usize],
    /// The number of lines to render for each item beginning above the screen index and rendering
    /// upwards.
    pub above: &'a [usize],
}

#[derive(Debug, Clone, Copy)]
struct ScreenAlignment {
    selection: u32,
    above: u16,
    size: u16,
    padding_top: u16,
    padding_bottom: u16,
}

impl ScreenAlignment {
    fn new(size: u16, padding_bottom: u16, padding_top: u16) -> Self {
        debug_assert!(padding_bottom + padding_top < size);
        Self {
            selection: 0,
            above: size,
            size,
            padding_top,
            padding_bottom,
        }
    }
}

impl ScreenAlignment {}

/// Stateful representation of the screen layout.
#[derive(Debug)]
pub struct Matcher {
    previous: ScreenAlignment,
    /// Whether or not the layout is 'reversed'.
    reversed: bool,
    /// The layout buffer below and including the matched item.
    below: Vec<usize>,
    /// The layout buffer above the matched item.
    above: Vec<usize>,
}

impl Matcher {
    fn reset_above(&mut self) {
        self.previous.above = self.previous.size - self.below.iter().sum::<usize>() as u16;
    }

    pub fn new(size: u16, padding_bottom: u16, padding_top: u16) -> Self {
        Self {
            previous: ScreenAlignment::new(size, padding_bottom, padding_top),
            below: Vec::with_capacity(size as usize),
            above: Vec::with_capacity(size as usize),
            reversed: false,
        }
    }

    pub fn selection(&self) -> u32 {
        self.previous.selection
    }

    pub fn selection_range(&self) -> std::ops::RangeInclusive<u32> {
        self.previous.selection + 1 - self.below.len() as u32
            ..=self.previous.selection + self.above.len() as u32
    }

    /// Get a representation of the current layout to be used for rendering.
    pub fn view(&self) -> LayoutView {
        LayoutView {
            below: &self.below,
            above: &self.above,
        }
    }

    /// Recompute the match layout when the screen size has changed.
    pub fn resize<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_bottom: u16,
        padding_top: u16,
        buffer: &B,
    ) {
        debug_assert!(padding_bottom + padding_top < total_size);

        // since the padding could change, make sure the value of 'above' is valid for the new
        // padding values
        self.previous.above = self
            .previous
            .above
            .clamp(padding_top, total_size - padding_bottom - 1);

        let sizes_below_incl = buffer.sizes_below(self.previous.selection, &mut self.below);
        let sizes_above = buffer.sizes_above(self.previous.selection, &mut self.above);

        if self.reversed {
            if self.previous.size <= total_size {
                todo!();
            } else {
                todo!();
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if self.previous.size <= total_size {
                resize::larger(self.previous, total_size, sizes_below_incl, sizes_above);
            } else {
                resize::smaller(
                    self.previous,
                    total_size,
                    padding_top,
                    sizes_below_incl,
                    sizes_above,
                );
            }
        }

        self.previous.size = total_size;
        self.previous.padding_bottom = padding_bottom;
        self.previous.padding_top = padding_top;
        self.reset_above();
    }

    /// Reset the layout, setting the cursor to '0' and rendering the items.
    pub fn reset<B: VariableSizeBuffer>(&mut self, buffer: &B) -> bool {
        if self.previous.selection != 0 {
            let sizes_below_incl = buffer.sizes_below(0, &mut self.below);
            if self.reversed {
                reset::reset_rev(self.previous.size, sizes_below_incl);
            } else {
                let sizes_above = buffer.sizes_above(0, &mut self.above);
                reset::reset(
                    self.previous.size,
                    self.previous.padding_top,
                    sizes_below_incl,
                    sizes_above,
                );
            }

            self.previous.selection = 0;
            self.reset_above();
            true
        } else {
            false
        }
    }

    /// Update the layout with the modified item list.
    pub fn update_items<B: VariableSizeBuffer>(&mut self, buffer: &B) {
        // clamp the previous cursor in case it has become invalid for the updated items
        self.previous.selection = self
            .previous
            .selection
            .min(buffer.total().saturating_sub(1));

        if buffer.total() > 0 {
            let sizes_below_incl = buffer.sizes_below(self.previous.selection, &mut self.below);
            let sizes_above = buffer.sizes_above(self.previous.selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
                update::items(self.previous, sizes_below_incl, sizes_above);
            }

            self.reset_above();
        } else {
            self.below.clear();
            self.above.clear();
            self.previous.selection = 0;
            self.reset_above();
        }
    }

    /// Increment the selection by the given amount.
    pub fn selection_incr<B: VariableSizeBuffer>(&mut self, increase: u32, buffer: &B) -> bool {
        let new_selection = self
            .previous
            .selection
            .saturating_add(increase)
            .min(buffer.total().saturating_sub(1));

        if new_selection != self.previous.selection {
            let sizes_below_incl = buffer.sizes_below(new_selection, &mut self.below);
            let sizes_above = buffer.sizes_above(new_selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
                selection::incr(self.previous, new_selection, sizes_below_incl, sizes_above);
            }

            self.previous.selection = new_selection;
            self.reset_above();

            true
        } else {
            false
        }
    }

    /// Decrement the selection by the given amount.
    pub fn selection_decr<B: VariableSizeBuffer>(&mut self, decrease: u32, buffer: &B) -> bool {
        let new_selection = self.previous.selection.saturating_sub(decrease);

        if new_selection != self.previous.selection {
            let sizes_below_incl = buffer.sizes_below(new_selection, &mut self.below);
            let sizes_above = buffer.sizes_above(new_selection, &mut self.above);

            if self.reversed {
                todo!()
            } else {
                selection::decr(self.previous, new_selection, sizes_below_incl, sizes_above);
            }

            self.previous.selection = new_selection;
            self.reset_above();

            true
        } else {
            false
        }
    }
}
