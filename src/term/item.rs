#[cfg(test)]
mod tests;

use std::{num::NonZero, ops::RangeBounds};

use memchr::memchr_iter;
use nucleo::{Item, Snapshot, Utf32Str};

use crate::Render;

/// A special buffer of items which need not have fixed widths.
pub trait VariableSizeBuffer {
    type Item<'a>
    where
        Self: 'a;

    /// The total number items contained in the buffer.
    fn count(&self) -> u32;

    /// Obtain an iterator of items in the given range.
    fn items(
        &self,
        range: impl RangeBounds<u32>,
    ) -> impl ExactSizeIterator<Item = Self::Item<'_>> + DoubleEndedIterator + '_;

    /// Compute the width of an item in the buffer.
    fn size(item: &Self::Item<'_>) -> NonZero<usize>;

    /// A convenience function to obtain an iterator of the item widths in the given range.
    fn item_sizes(&self, range: impl RangeBounds<u32>) -> impl DoubleEndedIterator<Item = usize> {
        self.items(range).map(|item| Self::size(&item).get())
    }
}

impl<T: Send + Sync + 'static> VariableSizeBuffer for Snapshot<T> {
    type Item<'a>
        = Item<'a, T>
    where
        Self: 'a;

    fn count(&self) -> u32 {
        self.matched_item_count()
    }

    fn items(
        &self,
        range: impl RangeBounds<u32>,
    ) -> impl ExactSizeIterator<Item = Self::Item<'_>> + DoubleEndedIterator + '_ {
        self.matched_items(range)
    }

    fn size(item: &Self::Item<'_>) -> NonZero<usize> {
        let num_linebreaks = match item.matcher_columns[0].slice(..) {
            Utf32Str::Ascii(bytes) => memchr_iter(b'\n', bytes).count(),
            Utf32Str::Unicode(chars) => {
                // TODO: there is an upstream Unicode handling issue in that windows-style newlines are
                // mapped to `\r` instead of `\n`. Therefore we count both the number of occurrences of
                // `\r` and `\n`. This handles mixed `\r\n` as well as `\n`, but returns the incorrect
                // value in the presence of free-standing carriage returns.
                chars
                    .iter()
                    .filter(|ch| **ch == '\n' || **ch == '\r')
                    .count()
            }
        };
        // SAFETY: we are adding 1 to a usize
        unsafe { NonZero::new_unchecked(1 + num_linebreaks) }
    }
}

/// A container type since a [`Render`] implementation might return a type which needs ownership.
///
/// For the given item, check the corresponding variant. If the variant is ASCII, that means we can
/// use much more efficient ASCII processing on rendering.
pub enum RenderedItem<'a, S> {
    Ascii(&'a str),
    Unicode(S),
}

impl<'a, S> RenderedItem<'a, S> {
    /// Initialize a new `RenderedItem` from an [`Item`] and a [`Render`] implementation.
    pub fn new<T, R>(item: &Item<'a, T>, renderer: &R) -> Self
    where
        R: Render<T, Str<'a> = S>,
    {
        if let Utf32Str::Ascii(bytes) = item.matcher_columns[0].slice(..) {
            RenderedItem::Ascii(unsafe { std::str::from_utf8_unchecked(bytes) })
        } else {
            RenderedItem::Unicode(renderer.render(item.data))
        }
    }
}

impl<S: AsRef<str>> AsRef<str> for RenderedItem<'_, S> {
    fn as_ref(&self) -> &str {
        match self {
            RenderedItem::Ascii(s) => s,
            RenderedItem::Unicode(u) => u.as_ref(),
        }
    }
}

/// A view into a [`Layout`] at a given point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutView<'a> {
    /// The number of lines to render for each item beginning below the screen index and rendering
    /// downwards.
    pub below: &'a [u16],
    /// The number of lines to render the selected item.
    pub current: u16,
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
    match_index: u32,
    /// The screen index.
    screen_index: u16,
    /// The layout buffer above the matched item.
    below_and_including: Vec<u16>,
    /// The layout buffer below and including the matched item.
    above: Vec<u16>,
}

impl Layout {
    /// Get a representation of the current layout to be used for rendering.
    fn view(&self) -> LayoutView {
        debug_assert!(self.below_and_including.iter().sum::<u16>() == self.screen_index + 1);
        LayoutView {
            below: &self.below_and_including[1..],
            current: self.below_and_including[0],
            above: &self.above,
        }
    }

    /// Extend the line space buffer from the given [`Item`] iterator. Returns the amount of
    /// remaining space at the end (if any).
    #[inline]
    fn extend_layout<I: Iterator<Item = usize>>(
        buffer: &mut Vec<u16>,
        remaining_space: u16,
        items: I,
    ) -> Option<u16> {
        Self::extend_layout_excess(buffer, remaining_space, items).ok()
    }

    /// Extend the line space buffer from the given [`Item`] iterator. Return either the amount of
    /// remaining space at the end in the `Ok` variant, or the amount by which the final item was
    /// truncated in the `Err` variant.
    #[inline]
    fn extend_layout_excess<I: Iterator<Item = usize>>(
        buffer: &mut Vec<u16>,
        mut remaining_space: u16,
        items: I,
    ) -> Result<u16, (usize, usize)> {
        for (idx, item) in items.enumerate() {
            let required_space = item;
            if required_space >= remaining_space.into() {
                if remaining_space != 0 {
                    buffer.push(remaining_space);
                }
                return Err((idx + 1, required_space - remaining_space as usize));
            }

            remaining_space -= required_space as u16;
            buffer.push(required_space as u16);
        }
        Ok(remaining_space)
    }

    /// Reset the screen index in case the screen size has changed.
    #[inline]
    fn clamp_indices<B: VariableSizeBuffer>(&mut self, size: u16, padding_top: u16, buffer: &B) {
        self.screen_index = self.screen_index.min(size - padding_top - 1);
        self.match_index = self.match_index.min(buffer.count().saturating_sub(1));
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
        selection: u32,
        buffer: &B,
    ) -> LayoutView {
        debug_assert!(padding_bottom + padding_top < total_size);
        debug_assert!(selection < buffer.count());
        self.clamp_indices(total_size, padding_top, buffer);

        self.below_and_including.clear();
        self.above.clear();
        self.screen_index = if selection >= self.match_index {
            self.recompute_above(total_size, padding_top, selection, buffer)
        } else {
            self.recompute_below(total_size, padding_bottom, padding_top, selection, buffer)
        };
        self.match_index = selection;

        debug_assert!(self.screen_index < total_size);
        let view = self.view();
        debug_assert!(
            view.above.iter().sum::<u16>() + view.below.iter().sum::<u16>() + view.current
                <= total_size
        );
        view
    }

    #[inline]
    fn recompute_above<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_top: u16,
        selection: u32,
        buffer: &B,
    ) -> u16 {
        // first, iterate downwards until one of the following happens:
        // 1. we run out of screen space
        // 2. we hit the current matched item
        let remaining_space_above = match Self::extend_layout(
            &mut self.below_and_including,
            total_size - padding_top,
            buffer.item_sizes(self.match_index..=selection).rev(),
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
                // `below_and_including` must be non-empty
                let threshold = (self.screen_index + 1)
                    .saturating_sub(*self.below_and_including.last().unwrap());
                let (remaining_space_below, remaining_space_above) =
                    if threshold < remaining_space_below {
                        (threshold, padding_top + remaining_space_below - threshold)
                    } else {
                        (remaining_space_below, padding_top)
                    };

                // extend below: we are guaranteed to not hit the bottom of the screen since the
                // amount of space above can only increase
                remaining_space_above
                    + Self::extend_layout(
                        &mut self.below_and_including,
                        remaining_space_below,
                        buffer.item_sizes(..self.match_index).rev(),
                    )
                    .unwrap_or(0)
            }
        };

        // extend above
        if selection < buffer.count() - 1 {
            Self::extend_layout(
                &mut self.above,
                remaining_space_above,
                buffer.item_sizes(selection + 1..),
            );
        }

        // set the screen index
        total_size - remaining_space_above - 1
    }

    #[inline]
    fn recompute_below<B: VariableSizeBuffer>(
        &mut self,
        total_size: u16,
        padding_bottom: u16,
        padding_top: u16,
        selection: u32,
        buffer: &B,
    ) -> u16 {
        // first, render as much of the selection as possible
        match Self::extend_layout(
            &mut self.below_and_including,
            total_size - padding_top,
            buffer.item_sizes(selection..=selection),
        ) {
            None => {
                // rendering the selection already took all the space, so we just render the top
                // padding
                Self::extend_layout(
                    &mut self.above,
                    padding_top,
                    buffer.item_sizes(selection + 1..),
                );

                total_size - padding_top - 1
            }
            Some(remaining) => {
                // there is leftover space: this is how much space the selection took
                let selection_size = total_size - padding_top - remaining;

                let (extra_rendered, total_bottom_size, bottom_item_excess) =
                    if selection_size > padding_bottom {
                        // the selection is fully rendered and large enough to fill the bottom
                        // padding
                        (0, selection_size, 0)
                    } else {
                        // the selection didn't completely fill the bottom padding, fill it and keep
                        // track of the extra space
                        match Self::extend_layout_excess(
                            &mut self.below_and_including,
                            padding_bottom - selection_size + 1,
                            buffer.item_sizes(..selection).rev(),
                        ) {
                            Ok(remaining_bottom_padding) => {
                                // we hit the bottom of the screen, so we just render the remaining
                                // space above and return early
                                Self::extend_layout(
                                    &mut self.above,
                                    total_size - padding_bottom - 1 + remaining_bottom_padding,
                                    buffer.item_sizes(selection + 1..),
                                );
                                return padding_bottom - remaining_bottom_padding;
                            }
                            Err((num_rendered, bottom_item_excess)) => {
                                (num_rendered, padding_bottom + 1, bottom_item_excess)
                            }
                        }
                    };

                // now we have completely filled the bottom padding, so we fill the space above
                // until we hit the match index
                total_bottom_size - 1
                    + match Self::extend_layout(
                        &mut self.above,
                        total_size - total_bottom_size,
                        buffer.item_sizes(selection + 1..=self.match_index),
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
                            if self.match_index + 1 < buffer.count() {
                                Self::extend_layout(
                                    &mut self.above,
                                    remaining_space_above,
                                    buffer.item_sizes(self.match_index + 1..),
                                );
                            }

                            // the excess size of the bottom item is already enough to
                            // cover the remaining space, so we just modify the last
                            // element of the layout
                            if bottom_item_excess >= remaining_space_below as usize {
                                // SAFETY: we've already rendered `selection`
                                *self.below_and_including.last_mut().unwrap() +=
                                    remaining_space_below;
                                remaining_space_below
                            } else {
                                // SAFETY: we've already rendered `selection` and bottom_item_excess <
                                // selection which is a u16
                                *self.below_and_including.last_mut().unwrap() +=
                                    bottom_item_excess as u16;
                                remaining_space_below
                                    - Self::extend_layout(
                                        &mut self.below_and_including,
                                        remaining_space_below - bottom_item_excess as u16,
                                        buffer
                                            .item_sizes(..selection - extra_rendered as u32)
                                            .rev(),
                                    )
                                    .unwrap_or(0)
                            }
                        }
                    }
            }
        }
    }
}
