use super::MatchListState;
use crate::incremental::ExtendIncremental;

#[inline]
pub fn items(
    previous: MatchListState,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // we want to preserve the value of `previous.above`; but this might fail if:
    // 1. we hit the start of the list when rendering below, or
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

#[inline]
pub fn items_rev(
    previous: MatchListState,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // we want to preserve the value of `previous.below`; but this might fail if:
    // 1. we hit the start of the list when rendering above, or
    // 2. the size of the selection is too large.

    let mut total_remaining = previous.size;

    // render the selection and any space above the selection, attempting to reserve
    // 'previous.below' space if possible
    let selection_size = sizes_below_incl.extend_bounded(total_remaining - padding_top, 1);
    total_remaining -= sizes_above
        .extend_unbounded(total_remaining.saturating_sub(previous.below.max(selection_size)));
    total_remaining -= selection_size;

    // render anything remaining below the selection
    sizes_below_incl.extend_unbounded(total_remaining);
}
