//! Match list layout
mod rank;
mod reset;
mod resize;
mod update;

use nucleo as nc;

use super::{
    MatchListConfig,
    item::{ItemList, ItemListExt},
};

/// Context from the previous render used to update the screen correctly.
#[derive(Debug)]
struct LayoutState {
    rank: u32,
    below: u16,
    above: u16,
    size: u16,
}

/// A description of how the match list is laid out on the screen.
pub(crate) struct ListLayout {
    /// The size of the screen last time the screen changed.
    size: u16,
    pub(super) rank: u32,
    /// The layout buffer below and including the hovered item.
    pub(super) below: Vec<usize>,
    /// The layout buffer above the hovered item.
    pub(super) above: Vec<usize>,
}

impl ListLayout {
    pub fn rank<T: Send + Sync + 'static>(&self, snapshot: &nc::Snapshot<T>) -> Option<u32> {
        (snapshot.matched_item_count() != 0).then_some(self.rank)
    }

    pub fn restart<T: Send + Sync + 'static>(
        &mut self,
        snapshot: &nc::Snapshot<T>,
        config: &MatchListConfig,
    ) {
        self.rank = 0;
        self.update_items(snapshot, config);
    }

    pub(crate) fn new() -> Self {
        Self {
            size: 0,
            rank: 0,
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
            rank: self.rank,
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
    pub fn visible_range(&self, config: &MatchListConfig) -> std::ops::RangeInclusive<usize> {
        if config.reversed {
            self.rank as usize - self.above.len()..=self.rank as usize + self.below.len() - 1
        } else {
            self.rank as usize + 1 - self.below.len()..=self.rank as usize + self.above.len()
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

            let sizes_below_incl = buffer.sizes_higher_inclusive(self.rank, &mut self.below);
            let sizes_above = buffer.sizes_lower(self.rank, &mut self.above);

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

            let sizes_below_incl = buffer.sizes_lower_inclusive(self.rank, &mut self.below);
            let sizes_above = buffer.sizes_higher(self.rank, &mut self.above);

            if self.size <= total_size {
                resize::larger(previous, total_size, sizes_below_incl, sizes_above);
            } else {
                resize::smaller(previous, total_size, padding, sizes_below_incl, sizes_above);
            }
        }

        self.size = total_size;
    }

    /// Reset the layout, setting the rank to '0' and rendering the items.
    pub fn reset<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        config: &MatchListConfig,
    ) -> bool {
        let padding = Self::padding(self.size, config);
        if self.rank != 0 {
            if config.reversed {
                let sizes_below_incl = buffer.sizes_higher_inclusive(0, &mut self.below);
                self.above.clear();

                reset::reset_rev(self.size, sizes_below_incl);
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(0, &mut self.below);
                let sizes_above = buffer.sizes_higher(0, &mut self.above);

                reset::reset(self.size, padding, sizes_below_incl, sizes_above);
            }

            self.rank = 0;
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
        // clamp the previous rank in case it has become invalid for the updated items
        self.rank = self.rank.min(buffer.total().saturating_sub(1));
        let previous = self.state();
        let padding = Self::padding(self.size, config);

        if buffer.total() > 0 {
            if config.reversed {
                let sizes_below_incl = buffer.sizes_higher_inclusive(self.rank, &mut self.below);
                let sizes_above = buffer.sizes_lower(self.rank, &mut self.above);

                update::items_rev(previous, padding, sizes_below_incl, sizes_above);
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(self.rank, &mut self.below);
                let sizes_above = buffer.sizes_higher(self.rank, &mut self.above);

                update::items(previous, padding, sizes_below_incl, sizes_above);
            }
        } else {
            self.below.clear();
            self.above.clear();
            self.rank = 0;
        }
    }

    /// Set the rank to a new value.
    #[inline]
    pub fn set_rank<T: Send + Sync + 'static>(
        &mut self,
        buffer: &nc::Snapshot<T>,
        new_rank: u32,
        config: &MatchListConfig,
    ) -> bool {
        let new_rank = new_rank.min(buffer.total().saturating_sub(1));

        let previous = self.state();
        let padding = Self::padding(self.size, config);

        if new_rank == 0 {
            self.reset(buffer, config)
        } else if new_rank > self.rank {
            if config.reversed {
                let sizes_below_incl = buffer.sizes_higher_inclusive(new_rank, &mut self.below);
                let sizes_above = buffer.sizes_lower(new_rank, &mut self.above);

                rank::incr_rev(
                    previous,
                    new_rank,
                    padding,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(new_rank, &mut self.below);
                let sizes_above = buffer.sizes_higher(new_rank, &mut self.above);

                rank::incr(previous, new_rank, padding, sizes_below_incl, sizes_above);
            }

            self.rank = new_rank;

            true
        } else if new_rank < self.rank {
            if config.reversed {
                let sizes_below_incl = buffer.sizes_higher_inclusive(new_rank, &mut self.below);
                let sizes_above = buffer.sizes_lower(new_rank, &mut self.above);

                rank::decr_rev(previous, new_rank, padding, sizes_below_incl, sizes_above);
            } else {
                let sizes_below_incl = buffer.sizes_lower_inclusive(new_rank, &mut self.below);
                let sizes_above = buffer.sizes_higher(new_rank, &mut self.above);

                rank::decr(
                    previous,
                    new_rank,
                    padding,
                    padding,
                    sizes_below_incl,
                    sizes_above,
                );
            }

            self.rank = new_rank;

            true
        } else {
            false
        }
    }
}
