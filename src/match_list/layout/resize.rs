use super::MatchListState;
use crate::incremental::ExtendIncremental;

#[inline]
pub fn larger(
    previous: MatchListState,
    mut total_remaining: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // fill the space below as far as possible
    total_remaining -= sizes_below_incl.extend_unbounded(total_remaining - previous.above);

    // and then anything remaining above: we use `total_remaining` rather than `previous.above`
    // since it is possible that we now hit the bottom of the screen in which case there is extra
    // space above
    sizes_above.extend_unbounded(total_remaining);
}

#[inline]
pub fn smaller(
    previous: MatchListState,
    mut total_remaining: u16,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // since the screen size changed, take the capacity from above, but do not exceed the top
    // padding
    let max_allowed_above = previous
        .above
        .saturating_sub(previous.size - total_remaining)
        .max(padding_top);

    // this is valid since the `previous.above` was already clamped
    let max_allowed_below = total_remaining - max_allowed_above;

    // first, render below: note that this is guaranteed to render as much of the selection as
    // possible since the selection size is unchanged, and we have first removed elements from
    // above as much as possible
    total_remaining -= sizes_below_incl.extend_unbounded(max_allowed_below);

    // then above
    sizes_above.extend_unbounded(total_remaining);
}

#[inline]
pub fn larger_rev(
    previous: MatchListState,
    mut total_remaining: u16,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    let new_size = total_remaining;

    // since the selection may have not fit with the previous screen size, try again to render as
    // much of the selection as possible
    total_remaining -= sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);

    // then render into the new space above
    total_remaining -= sizes_above.extend_unbounded(total_remaining.min(new_size - previous.below));

    // and then any more space below
    sizes_below_incl.extend_unbounded(total_remaining);
}

#[inline]
pub fn smaller_rev(
    previous: MatchListState,
    mut total_remaining: u16,
    padding_top: u16,
    padding_bottom: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // the amount that the screen decreased by
    let screen_delta = previous.size - total_remaining;

    // render as much of the selection as possible
    let selection_size = sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);

    // since the screen size changed, take the capacity from below, but do not exceed the bottom
    // padding or the selection size; take the remaining capacity from above
    let max_allowed_below = previous
        .below
        .saturating_sub(screen_delta)
        .max(padding_bottom + 1)
        .max(selection_size);
    let max_allowed_above = total_remaining - max_allowed_below;

    // and then above
    total_remaining -= selection_size;
    total_remaining -= sizes_above.extend_unbounded(max_allowed_above);

    // and then any of the remaining space below
    sizes_below_incl.extend_unbounded(total_remaining);
}
