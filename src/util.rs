/// Convert a type into a [`usize`], falling back to [`usize::MAX`] if it fails. This is mainly
/// used for converting `u32 -> usize` and will compile down to a no-op on the vast majority of
/// machines.
#[inline]
pub fn as_usize<T: TryInto<usize>>(num: T) -> usize {
    num.try_into().unwrap_or(usize::MAX)
}

/// Convert a type into a [`u32`], falling back to [`u32::MAX`] if it fails. This is mainly
/// used for converting `usize -> u32`.
#[inline]
pub fn as_u32<T: TryInto<u32>>(num: T) -> u32 {
    num.try_into().unwrap_or(u32::MAX)
}

#[inline]
pub fn as_u16<T: TryInto<u16>>(num: T) -> u16 {
    num.try_into().unwrap_or(u16::MAX)
}
