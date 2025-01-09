use super::MatchListState;
use crate::{incremental::ExtendIncremental, util::as_usize};

#[inline]
pub fn incr(
    previous: MatchListState,
    cursor: u32,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render new elements strictly above the previous selection
    let new_size_above = sizes_below_incl.extend_bounded(
        total_remaining - padding_top,
        as_usize(cursor - previous.selection),
    );
    total_remaining -= new_size_above;

    // subtract the newly rendered items from the space above; but do not exceed the top padding
    let max_allowed_above = previous
        .above
        .saturating_sub(new_size_above)
        .max(padding_top);

    // render the remaining elements: we are guaranteed to not hit the bottom of the screen since
    // the number of items rendered above in total can only increase
    sizes_above.extend_unbounded(max_allowed_above);
    sizes_below_incl.extend_unbounded(total_remaining - max_allowed_above);
}

#[inline]
pub fn decr(
    previous: MatchListState,
    cursor: u32,
    padding_top: u16,
    padding_bottom: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render as much of the selection as possible
    let selection_rendered = sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);
    total_remaining -= selection_rendered;

    // also try to fill the bottom padding
    total_remaining -=
        sizes_below_incl.extend_unbounded((padding_bottom + 1).saturating_sub(selection_rendered));

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

#[inline]
pub fn incr_rev(
    previous: MatchListState,
    cursor: u32,
    padding_top: u16,
    padding_bottom: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render as much of the selection as possible
    let selection_rendered = sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);
    total_remaining -= selection_rendered;

    // render above above until we hit the previous selection, without also filling the bottom
    // padding
    let rendered_above = sizes_above.extend_bounded(
        total_remaining.min(previous.size - padding_bottom - 1),
        as_usize(cursor - previous.selection),
    );
    total_remaining -= rendered_above;

    // compute the maximum amount of space above by taking the previous size and subtracting the
    // amount of space the new items rendered below occupy, making sure to also reserve space
    // for the bottom padding
    let max_space_above = previous.size
        - (rendered_above + selection_rendered.max(padding_bottom + 1)).max(previous.below);

    // render above; note that `max_space_above <= total_remaining` since we only restrict the size
    // more
    total_remaining -= sizes_above.extend_unbounded(max_space_above);

    // render anything remaining
    sizes_below_incl.extend_unbounded(total_remaining);
}

#[inline]
pub fn decr_rev(
    previous: MatchListState,
    cursor: u32,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let mut total_remaining = previous.size;

    // render new elements strictly above the previous selection
    let new_size_above = sizes_below_incl.extend_bounded(
        total_remaining - padding_top,
        as_usize(previous.selection - cursor),
    );
    total_remaining -= new_size_above;

    // subtract space from the previous space above, but do not go below the top padding
    let max_space_above = (previous.size - previous.below)
        .saturating_sub(new_size_above)
        .max(padding_top);

    total_remaining -= sizes_above.extend_unbounded(max_space_above);
    sizes_below_incl.extend_unbounded(total_remaining);
}
