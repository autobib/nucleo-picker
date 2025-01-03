use super::ScreenAlignment;
use crate::incremental::ExtendIncremental;

#[inline]
pub fn larger(
    previous: ScreenAlignment,
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
    previous: ScreenAlignment,
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
