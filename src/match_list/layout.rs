//! Match list layout
mod reset;
mod resize;
mod selection;
mod update;

use nucleo as nc;

use super::{
    MatchListConfig,
    item::{ItemList, ItemListExt},
};

/// Context from the previous render used to update the screen correctly.
#[derive(Debug)]
struct LayoutState {
    selection: u32,
    below: u16,
    above: u16,
    size: u16,
}

/// A description of how the match list is laid out on the screen.
pub(crate) struct ListLayout {
    /// The size of the screen last time the screen changed.
    size: u16,
    pub(super) selection: u32,
    /// The layout buffer below and including the matched item.
    pub(super) below: Vec<usize>,
    /// The layout buffer above the matched item.
    pub(super) above: Vec<usize>,
}

impl ListLayout {
    pub fn selection<T: Send + Sync + 'static>(&self, snapshot: &nc::Snapshot<T>) -> Option<u32> {
        (snapshot.matched_item_count() != 0).then_some(self.selection)
    }

    pub fn restart<T: Send + Sync + 'static>(
        &mut self,
        snapshot: &nc::Snapshot<T>,
        config: &MatchListConfig,
    ) {
        self.selection = 0;
        self.update_items(snapshot, config);
    }

    pub(crate) fn new() -> Self {
        Self {
            size: 0,
            selection: 0,
            below: Vec::with_capacity(128),
            above: Vec::with_capacity(128),
        }
    }

    /// Returns a self-contained representation of the screen state required for correct layout
    /// update computations.
    fn state(&self) -> LayoutState {
        let below = self.below.iter().sum::<usize>() as u16;
        let above = self.above.iter().sum::<usize>() as u16;
        LayoutState {
            selection: self.selection,
            below: self.size - above,
            above: self.size - below,
            size: self.size,
        }
    }

    /// The total amount of whitespace present in the displayed match list.
    pub(super) fn whitespace(&self) -> u16 {
        self.size
            - self.below.iter().sum::<usize>() as u16
            - self.above.iter().sum::<usize>() as u16
    }

    /// The amount of padding corresponding to the provided size.
    fn padding(size: u16, config: &MatchListConfig) -> u16 {
        config.scroll_padding.min(size.saturating_sub(1) / 2)
    }

    /// Return the range corresponding to the matched items visible on the screen.
    pub fn selection_range(&self, config: &MatchListConfig) -> std::ops::RangeInclusive<usize> {
        if config.reversed {
            self.selection as usize - self.above.len()
                ..=self.selection as usize + self.below.len() - 1
        } else {
            self.selection as usize + 1 - self.below.len()
                ..=self.selection as usize + self.above.len()
        }
    }

    /// Recompute the match layout when the screen size has changed.
    pub fn resize<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        total_size: u16,
        config: &MatchListConfig,
    ) {
        // no resize needed
        if total_size == self.size {
            return;
        }

        // check for zero, so the 'clamp' call dows not fail
        if total_size == 0 {
            self.size = 0;
            self.above.clear();
            self.below.clear();
            return;
        }

        // check for no elements, so the `sizes_below` and `sizes_above` calls do not fail
        if buffer.total() == 0 {
            self.size = total_size;
            return;
        }

        let padding = Self::padding(total_size, config);

        let mut previous = self.state();

        if config.reversed {
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

    /// Reset the layout, setting the cursor to '0' and rendering the items.
    pub fn reset<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        config: &MatchListConfig,
    ) -> bool {
        let padding = Self::padding(self.size, config);
        if self.selection != 0 {
            if config.reversed {
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
    pub fn update_items<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        config: &MatchListConfig,
    ) {
        // clamp the previous cursor in case it has become invalid for the updated items
        self.selection = self.selection.min(buffer.total().saturating_sub(1));
        let previous = self.state();
        let padding = Self::padding(self.size, config);

        if buffer.total() > 0 {
            if config.reversed {
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

    /// Set the selection to a new value.
    #[inline]
    pub fn set_selection<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        new_selection: u32,
        config: &MatchListConfig,
    ) -> bool {
        let new_selection = new_selection.min(buffer.total().saturating_sub(1));

        let previous = self.state();
        let padding = Self::padding(self.size, config);

        if new_selection == 0 {
            self.reset(buffer, config)
        } else if new_selection > self.selection {
            if config.reversed {
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
            if config.reversed {
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
    pub fn selection_incr<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        increase: u32,
        config: &MatchListConfig,
    ) -> bool {
        let new_selection = self
            .selection
            .saturating_add(increase)
            .min(buffer.total().saturating_sub(1));

        self.set_selection(buffer, new_selection, config)
    }

    /// Decrement the selection by the given amount.
    #[cfg(test)]
    pub fn selection_decr<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        decrease: u32,
        config: &MatchListConfig,
    ) -> bool {
        let new_selection = self.selection.saturating_sub(decrease);

        self.set_selection(buffer, new_selection, config)
    }
}
