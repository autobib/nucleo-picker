//! # Column fill helper functions
//! This module contains helper functions that are intended to be used as the `fill_columns`
//! argument for the [`Injector::push`](crate::nucleo::Injector::push) function.
//! It is straightforward to write such a function yourself if you want to customize the fill
//! behaviour in any way.
use nucleo::Utf32String;

/// Fill an item into the first column using its [`ToString`] or [`Display`](std::fmt::Display)
/// implementation.
pub fn fill_as_string<I: ToString>(item: &I, cols: &mut [Utf32String]) {
    cols[0] = item.to_string().into();
}
