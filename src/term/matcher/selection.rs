use super::ScreenAlignment;
use crate::{incremental::ExtendIncremental, util::as_usize};

#[inline]
pub fn incr(
    previous: ScreenAlignment,
    cursor: u32,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render new elements strictly above the previous selection
    let new_size_above = sizes_below_incl.extend_bounded(
        total_remaining - previous.padding_top,
        as_usize(cursor - previous.selection),
    );
    total_remaining -= new_size_above;

    // subtract the newly rendered items from the space above; but do not exceed the top padding
    let max_allowed_above = previous
        .above
        .saturating_sub(new_size_above)
        .max(previous.padding_top);

    // render the remaining elements: we are guaranteed to not hit the bottom of the screen since
    // the number of items rendered above in total can only increase
    sizes_above.extend_unbounded(max_allowed_above);
    sizes_below_incl.extend_unbounded(total_remaining - max_allowed_above);
}

#[inline]
pub fn decr(
    previous: ScreenAlignment,
    cursor: u32,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render as much of the selection as possible
    let selection_rendered =
        sizes_below_incl.extend_bounded(total_remaining - previous.padding_top, 1);
    total_remaining -= selection_rendered;

    // also try to fill the bottom padding
    total_remaining -= sizes_below_incl
        .extend_unbounded((previous.padding_bottom + 1).saturating_sub(selection_rendered));

    // render above above until we hit the previous selection
    total_remaining -=
        sizes_above.extend_bounded(total_remaining, as_usize(previous.selection - cursor));

    // truncate below to prevent the screen from scrolling unnecessarily
    let max_space_below = total_remaining - total_remaining.min(previous.above);

    // render any remaining space below
    total_remaining -= sizes_below_incl.extend_unbounded(max_space_below);

    // render above
    sizes_above.extend_unbounded(total_remaining);
}
