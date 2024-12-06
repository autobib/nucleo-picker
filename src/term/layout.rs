#[cfg(test)]
mod tests;

// pub trait Queue {
//     fn append(&mut self, width: u16);

//     fn increase_last(&mut self, incr: u16);

//     fn last(&self) -> u16;
// }

// impl Queue for Vec<u16> {
//     fn append(&mut self, width: u16) {
//         self.push(width);
//     }

//     fn increase_last(&mut self, incr: u16) {
//         *self.last_mut().unwrap() += incr;
//     }

//     fn last(&self) -> u16 {
//         std::vec::Vec::last(self)
//     }
// }

pub trait Cursor {
    fn index(self) -> u32;
}

impl Cursor for u32 {
    fn index(self) -> u32 {
        self
    }
}

/// A special buffer of items which need not have fixed widths.
pub trait VariableSizeBuffer {
    type Cursor: Cursor + Copy;

    /// The item type of the buffer.
    type Item<'a>
    where
        Self: 'a;

    /// The total number items contained in the buffer.
    fn count(&self) -> u32;

    /// Compute the width of an item in the buffer.
    fn size(item: &Self::Item<'_>) -> usize;

    /// An iterator over items below the cursor, iterating downwards.
    fn before(&self, cursor: Self::Cursor) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above cursor, iterating upwards.
    fn after(&self, cursor: Self::Cursor) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// A convenience function to iterate over item sizes corresponding to items returned by
    /// [`below`](VariableSizeBuffer::below).
    fn sizes_before(&self, cursor: Self::Cursor) -> impl DoubleEndedIterator<Item = usize> {
        self.before(cursor).map(|item| Self::size(&item))
    }

    /// A convenience function to iterate over item sizes corresponding to items returned by
    /// [`above`](VariableSizeBuffer::above).
    fn sizes_after(&self, cursor: Self::Cursor) -> impl DoubleEndedIterator<Item = usize> {
        self.after(cursor).map(|item| Self::size(&item))
    }
}

/// A view into a [`Layout`] at a given point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutView<'a> {
    /// The number of lines to render for each item beginning below the screen index and rendering
    /// downwards.
    pub below: &'a [u16],
    // /// The number of lines to render the selected item.
    // pub current: u16,
    /// The number of lines to render for each item beginning above the screen index and rendering
    /// upwards.
    pub above: &'a [u16],
}

/// Stateful representation of the screen layout.
///
/// The layout is top-biased: when there is a limited amount of space and the given item is very
/// large, prefer placing the cursor at a position which shows the top lines instead of the bottom
/// lines.
#[derive(Debug, Default)]
pub struct Layout {
    /// The match index.
    previous_cursor: u32,
    /// The screen index.
    screen_index: u16,
    /// The layout buffer below and including the matched item.
    below: Vec<u16>,
    /// The layout buffer above the matched item.
    above: Vec<u16>,
}

impl Layout {
    /// Get a representation of the current layout to be used for rendering.
    fn view(&self) -> LayoutView {
        debug_assert!(self.below.iter().sum::<u16>() == self.screen_index + 1);
        LayoutView {
            below: &self.below,
            above: &self.above,
        }
    }

    /// Extend the line space buffer from the given [`Item`] iterator. Returns the amount of
    /// remaining space at the end (if any).
    #[inline]
    fn extend_layout<I: Iterator<Item = usize>>(
        buffer: &mut Vec<u16>,
        remaining_size: u16,
        required_sizes: I,
    ) -> Option<u16> {
        Self::extend_layout_excess(buffer, remaining_size, required_sizes).ok()
    }

    /// Extend the line space buffer from the given [`Item`] iterator. Return either the amount of
    /// remaining space at the end in the `Ok` variant, or the amount by which the final item was
    /// truncated in the `Err` variant.
    #[inline]
    fn extend_layout_excess<I: Iterator<Item = usize>>(
        buffer: &mut Vec<u16>,
        mut remaining_size: u16,
        required_sizes: I,
    ) -> Result<u16, usize> {
        for size in required_sizes {
            if size >= remaining_size.into() {
                if remaining_size != 0 {
                    buffer.push(remaining_size);
                }
                return Err(size - remaining_size as usize);
            }

            remaining_size -= size as u16;
            buffer.push(size as u16);
        }
        Ok(remaining_size)
    }

    /// Reset the screen index in case the screen size has changed.
    #[inline]
    fn clamp_indices<B: VariableSizeBuffer>(&mut self, size: u16, padding_top: u16, buffer: &B) {
        self.screen_index = self.screen_index.min(size - padding_top - 1);
        self.previous_cursor = self.previous_cursor.min(buffer.count().saturating_sub(1));
    }

    /// Recompute the internal layout given a selection index, which will become the new match
    /// index after the method is completed.
    ///
    /// This method is used to process actions such as moving the cursor up and down. Since we
    /// process keyboard input in batches, this method is designed to allow arbitrary changes
    /// in the selection.
    ///
    /// After recomputing, return a view of the internal buffers to use when rendering the screen.
    #[must_use]
    pub fn recompute<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_bottom: u16,
        padding_top: u16,
        cursor: <B as VariableSizeBuffer>::Cursor,
        buffer: &B,
    ) -> LayoutView {
        debug_assert!(padding_bottom + padding_top < total_size);
        debug_assert!(cursor.index() < buffer.count());
        self.clamp_indices(total_size, padding_top, buffer);

        self.below.clear();
        self.above.clear();
        self.screen_index = if cursor.index() >= self.previous_cursor {
            self.recompute_above(total_size, padding_top, cursor, buffer)
        } else {
            self.recompute_below(total_size, padding_bottom, padding_top, cursor, buffer)
        };
        self.previous_cursor = cursor.index();

        debug_assert!(self.screen_index < total_size);
        let view = self.view();
        debug_assert!(
            view.above.iter().sum::<u16>() + view.below.iter().sum::<u16>() <= total_size
        );
        view
    }

    #[inline]
    fn recompute_above<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_top: u16,
        cursor: <B as VariableSizeBuffer>::Cursor,
        buffer: &B,
    ) -> u16 {
        let mut sizes_below_incl = buffer.sizes_before(cursor);
        let sizes_above = buffer.sizes_after(cursor);

        // first, iterate downwards until one of the following happens:
        // 1. we run out of screen space
        // 2. we hit the current matched item
        let remaining_space_above = match Self::extend_layout(
            &mut self.below,
            total_size - padding_top,
            sizes_below_incl
                .by_ref()
                .take((cursor.index() - self.previous_cursor + 1) as usize),
        ) {
            None => {
                // we ran out of space, so we fill the space above, which is just `padding_top`.
                padding_top
            }
            Some(remaining_space_below) => {
                // truncate the amount of remaining space below to not exceed the previous space
                // below (which is exactly `self.screen_index`) to prevent the screen from scrolling
                // down unnecessarily

                // SAFETY: we had space left over and we tried to add at least one element, so
                // `below` must be non-empty
                let threshold = (self.screen_index + 1).saturating_sub(*self.below.last().unwrap());
                let (remaining_space_below, remaining_space_above) =
                    if threshold < remaining_space_below {
                        (threshold, padding_top + remaining_space_below - threshold)
                    } else {
                        (remaining_space_below, padding_top)
                    };

                // extend below: we are guaranteed to not hit the bottom of the screen since the
                // amount of space above can only increase
                remaining_space_above
                    + Self::extend_layout(&mut self.below, remaining_space_below, sizes_below_incl)
                        .unwrap_or(0)
            }
        };

        // extend above
        Self::extend_layout(&mut self.above, remaining_space_above, sizes_above);

        // set the screen index
        total_size - remaining_space_above - 1
    }

    #[inline]
    fn recompute_below<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_bottom: u16,
        padding_top: u16,
        cursor: <B as VariableSizeBuffer>::Cursor,
        buffer: &B,
    ) -> u16 {
        let mut sizes_below_incl = buffer.sizes_before(cursor);
        let mut sizes_above = buffer.sizes_after(cursor);

        // first, render as much of the selection as possible
        match Self::extend_layout(
            &mut self.below,
            total_size - padding_top,
            sizes_below_incl.by_ref().take(1),
        ) {
            None => {
                // rendering the cursor already took all the space, so we just render the top
                // padding
                Self::extend_layout(&mut self.above, padding_top, sizes_above);

                total_size - padding_top - 1
            }
            Some(remaining) => {
                // there is leftover space: this is how much space the cursor took
                let selection_size = total_size - padding_top - remaining;

                let (total_bottom_size, bottom_item_excess) = if selection_size > padding_bottom {
                    // the selection is fully rendered and large enough to fill the bottom
                    // padding
                    (selection_size, 0)
                } else {
                    // the selection didn't completely fill the bottom padding, fill it and keep
                    // track of the extra space
                    match Self::extend_layout_excess(
                        &mut self.below,
                        padding_bottom - selection_size + 1,
                        sizes_below_incl.by_ref(),
                    ) {
                        Ok(remaining_bottom_padding) => {
                            // we hit the bottom of the screen, so we just render the remaining
                            // space above and return early
                            Self::extend_layout(
                                &mut self.above,
                                total_size - padding_bottom - 1 + remaining_bottom_padding,
                                sizes_above,
                            );
                            return padding_bottom - remaining_bottom_padding;
                        }
                        Err(bottom_item_excess) => (padding_bottom + 1, bottom_item_excess),
                    }
                };

                // now we have completely filled the bottom padding, so we fill the space above
                // until we hit the match index
                total_bottom_size - 1
                    + match Self::extend_layout(
                        &mut self.above,
                        total_size - total_bottom_size,
                        sizes_above
                            .by_ref()
                            .take((self.previous_cursor - cursor.index()) as usize),
                        // buffer.item_sizes(selection + 1..=self.match_index),
                    ) {
                        None => {
                            // we ran out of space, so we're done; the bottom padding is already
                            // filled
                            0
                        }
                        Some(remaining_space) => {
                            // truncate the amount of remaining space above to not exceed the previous
                            // space above (which is exactly `total_size - self.screen_index - 1`) to
                            // prevent the screen from scrolling up unnecessarily
                            let max_space_above = total_size - self.screen_index - 1;
                            let (remaining_space_below, remaining_space_above) =
                                if max_space_above < remaining_space {
                                    (remaining_space - max_space_above, max_space_above)
                                } else {
                                    (0, remaining_space)
                                };

                            // render above
                            if self.previous_cursor + 1 < buffer.count() {
                                Self::extend_layout(
                                    &mut self.above,
                                    remaining_space_above,
                                    sizes_above,
                                );
                            }

                            // the excess size of the bottom item is already enough to
                            // cover the remaining space, so we just modify the last
                            // element of the layout
                            if bottom_item_excess >= remaining_space_below as usize {
                                // SAFETY: we've already rendered `selection`
                                *self.below.last_mut().unwrap() += remaining_space_below;
                                remaining_space_below
                            } else {
                                // SAFETY: we've already rendered `selection` and bottom_item_excess <
                                // selection which is a u16
                                *self.below.last_mut().unwrap() += bottom_item_excess as u16;
                                remaining_space_below
                                    - Self::extend_layout(
                                        &mut self.below,
                                        remaining_space_below - bottom_item_excess as u16,
                                        sizes_below_incl,
                                    )
                                    .unwrap_or(0)
                            }
                        }
                    }
            }
        }
    }
}
