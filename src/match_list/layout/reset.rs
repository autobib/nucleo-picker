use crate::incremental::ExtendIncremental;

#[inline]
pub fn reset(
    mut total_remaining: u16,
    padding_top: u16,
    mut sizes_below_incl: impl ExtendIncremental,
    mut sizes_above: impl ExtendIncremental,
) {
    // cursor = 0, so this renders exactly one element; but we need to make sure we do not
    // accidentally fill `padding_top` as well
    total_remaining -= sizes_below_incl.extend_unbounded(total_remaining - padding_top);
    sizes_above.extend_unbounded(total_remaining);
}

#[inline]
pub fn reset_rev(total_remaining: u16, mut sizes_below_incl: impl ExtendIncremental) {
    sizes_below_incl.extend_unbounded(total_remaining);
}
