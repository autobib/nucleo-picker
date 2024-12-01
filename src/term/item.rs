#[cfg(test)]
mod tests;

use std::num::NonZero;

use memchr::memchr_iter;
use nucleo::{Item, Snapshot, Utf32Str};

use crate::Render;

/// A container type since a [`Render`] implementation might return a type which needs ownership.
///
/// For the given item, check the corresponding variant. If the variant is ASCII, that means we can
/// use much more efficient ASCII processing on rendering.
pub enum Rendered<'a, S> {
    Ascii(&'a str),
    Unicode(S),
}

pub fn new_rendered<'a, T, R: Render<T>>(
    item: &Item<'a, T>,
    renderer: &R,
) -> Rendered<'a, <R as Render<T>>::Str<'a>> {
    if let Utf32Str::Ascii(bytes) = item.matcher_columns[0].slice(..) {
        Rendered::Ascii(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        Rendered::Unicode(renderer.render(item.data))
    }
}

impl<S: AsRef<str>> AsRef<str> for Rendered<'_, S> {
    fn as_ref(&self) -> &str {
        match self {
            Rendered::Ascii(s) => s,
            Rendered::Unicode(u) => u.as_ref(),
        }
    }
}

/// Determine how many lines it will take to render a given item.
fn count_lines<T>(item: &Item<'_, T>) -> NonZero<usize> {
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

/// A representation of the screen layout.
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

/// A representation of the internal screen layout.
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
    fn extend_layout<'a, T: Send + Sync + 'static, I: Iterator<Item = Item<'a, T>>>(
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
    fn extend_layout_excess<'a, T: Send + Sync + 'static, I: Iterator<Item = Item<'a, T>>>(
        buffer: &mut Vec<u16>,
        mut remaining_space: u16,
        items: I,
    ) -> Result<u16, (usize, usize)> {
        for (idx, item) in items.enumerate() {
            let required_space = count_lines(&item).get();
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
    fn clamp_indices<T: Send + Sync + 'static>(
        &mut self,
        height: u16,
        margin_top: u16,
        snapshot: &Snapshot<T>,
    ) {
        self.screen_index = self.screen_index.min(height - margin_top - 1);
        self.match_index = self
            .match_index
            .min(snapshot.matched_item_count().saturating_sub(1));
    }

    #[must_use]
    pub fn recompute<T: Send + Sync + 'static>(
        &mut self,
        height: u16,
        margin_bottom: u16,
        margin_top: u16,
        selection: u32,
        snapshot: &Snapshot<T>,
    ) -> LayoutView {
        debug_assert!(margin_bottom + margin_top < height);
        debug_assert!(selection < snapshot.matched_item_count());
        self.clamp_indices(height, margin_top, snapshot);

        self.below_and_including.clear();
        self.above.clear();
        self.screen_index = if selection >= self.match_index {
            self.recompute_above(height, margin_top, selection, snapshot)
        } else {
            self.recompute_below(height, margin_bottom, margin_top, selection, snapshot)
        };
        self.match_index = selection;

        debug_assert!(self.screen_index < height);
        let view = self.view();
        debug_assert!(
            view.above.iter().sum::<u16>() + view.below.iter().sum::<u16>() + view.current
                <= height
        );
        view
    }

    #[inline]
    fn recompute_above<T: Send + Sync + 'static>(
        &mut self,
        height: u16,
        margin_top: u16,
        selection: u32,
        snapshot: &Snapshot<T>,
    ) -> u16 {
        // first, iterate downwards until one of the following happens:
        // 1. we run out of screen space
        // 2. we hit the current matched item
        let remaining_space_above = match Self::extend_layout(
            &mut self.below_and_including,
            height - margin_top,
            snapshot.matched_items(self.match_index..=selection).rev(),
        ) {
            None => {
                // we ran out of space, so we fill the space above, which is just `margin_top`.
                margin_top
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
                        (threshold, margin_top + remaining_space_below - threshold)
                    } else {
                        (remaining_space_below, margin_top)
                    };

                // extend below: we are guaranteed to not hit the bottom of the screen since the
                // amount of space above can only increase
                remaining_space_above
                    + Self::extend_layout(
                        &mut self.below_and_including,
                        remaining_space_below,
                        snapshot.matched_items(..self.match_index).rev(),
                    )
                    .unwrap_or(0)
            }
        };

        // extend above
        if selection < snapshot.matched_item_count() - 1 {
            Self::extend_layout(
                &mut self.above,
                remaining_space_above,
                snapshot.matched_items(selection + 1..),
            );
        }

        // set the screen index
        height - remaining_space_above - 1
    }

    #[inline]
    fn recompute_below<T: Send + Sync + 'static>(
        &mut self,
        height: u16,
        margin_bottom: u16,
        margin_top: u16,
        selection: u32,
        snapshot: &Snapshot<T>,
    ) -> u16 {
        // first, render as much of the selection as possible
        match Self::extend_layout(
            &mut self.below_and_including,
            height - margin_top,
            snapshot.matched_items(selection..=selection),
        ) {
            None => {
                // rendering the selection already took all the space, so we just render the top
                // margin
                Self::extend_layout(
                    &mut self.above,
                    margin_top,
                    snapshot.matched_items(selection + 1..),
                );

                height - margin_top - 1
            }
            Some(remaining) => {
                // there is leftover space: this is how much space the selection took
                let selection_height = height - margin_top - remaining;

                let (extra_rendered, total_bottom_height, bottom_item_excess) =
                    if selection_height > margin_bottom {
                        // the selection is fully rendered and large enough to fill the bottom margin
                        (0, selection_height, 0)
                    } else {
                        // the selection didn't completely fill the bottom margin, fill it and keep
                        // track of the extra space
                        match Self::extend_layout_excess(
                            &mut self.below_and_including,
                            margin_bottom - selection_height + 1,
                            snapshot.matched_items(..selection).rev(),
                        ) {
                            Ok(remaining_bottom_margin) => {
                                // we hit the bottom of the screen, so we just render the remaining
                                // space above and return early
                                Self::extend_layout(
                                    &mut self.above,
                                    height - margin_bottom - 1 + remaining_bottom_margin,
                                    snapshot.matched_items(selection + 1..),
                                );
                                return margin_bottom - remaining_bottom_margin;
                            }
                            Err((num_rendered, bottom_item_excess)) => {
                                (num_rendered, margin_bottom + 1, bottom_item_excess)
                            }
                        }
                    };

                // now we have completely filled the bottom margin, so we fill the space above
                // until we hit the match index
                total_bottom_height - 1
                    + match Self::extend_layout(
                        &mut self.above,
                        height - total_bottom_height,
                        snapshot.matched_items(selection + 1..=self.match_index),
                    ) {
                        None => {
                            // we ran out of space, so we're done; the bottom margin is already
                            // filled
                            0
                        }
                        Some(remaining_space) => {
                            // truncate the amount of remaining space above to not exceed the previous
                            // space above (which is exactly `height - self.screen_index - 1`) to
                            // prevent the screen from scrolling up unnecessarily
                            let max_space_above = height - self.screen_index - 1;
                            let (remaining_space_below, remaining_space_above) =
                                if max_space_above < remaining_space {
                                    (remaining_space - max_space_above, max_space_above)
                                } else {
                                    (0, remaining_space)
                                };

                            // render above
                            if self.match_index + 1 < snapshot.matched_item_count() {
                                Self::extend_layout(
                                    &mut self.above,
                                    remaining_space_above,
                                    snapshot.matched_items(self.match_index + 1..),
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
                                        snapshot
                                            .matched_items(..selection - extra_rendered as u32)
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
