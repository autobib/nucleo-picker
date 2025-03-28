//! # Filters for use in a [`Render`](super::Render)
//!
//! This module contains built-in renderers to be used in a [`Picker`](super::Picker), and (with
//! appropriate types) can be used as the arguments passed to the
//! [`PickerOptions::picker`](super::PickerOptions::picker) and [`Picker::new`](super::Picker::new)
//! methods.

/// A renderer for any type which de-references as [`str`], such as a [`String`].
///
/// ## Example
/// ```
/// # use nucleo_picker::{render::StrRenderer, Render};
/// let str_renderer = StrRenderer;
///
/// let st = "Hello!".to_owned();
///
/// assert_eq!(str_renderer.render(&st), "Hello!");
/// ```

pub fn filter_control_chars_immutable(input: &str) -> String {
    input
        .chars()
        .filter(|&c| !c.is_control()) // Filter out control characters
        .collect() // Collect the non-control characters into a new String
}

pub fn filter_control_chars_inplace(s: &mut String) {
    // Retain only the characters that are not control characters
    s.retain(|c| !c.is_control());
}
