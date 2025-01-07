use super::Previous;
use crate::incremental::ExtendIncremental;

#[inline]
pub fn items(
    previous: Previous,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // we want to preserve the value of `previous.above`; but this might fail if:
    // 1. we hit the bottom of the screen when rendering below, or
    // 2. the size of the selection is too large.

    let mut total_remaining = previous.size;

    // render the selection
    total_remaining -= sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);

    // render any space below the selection, attempting to reserve 'previous.above' space if
    // possible
    total_remaining -=
        sizes_below_incl.extend_unbounded(total_remaining.saturating_sub(previous.above));

    // render anything remaining above the selection
    sizes_above.extend_unbounded(total_remaining);
}
