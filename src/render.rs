//! # Renderers for use in a [`Picker`](super::Picker)
//!
//! This module contains built-in renderers to be used in a [`Picker`](super::Picker), and (with
//! appropriate types) can be used as the arguments passed to the
//! [`PickerOptions::picker`](super::PickerOptions::picker) and [`Picker::new`](super::Picker::new)
//! methods.

pub use nucleo_picker_core::render::{DisplayRenderer, PathRenderer, StrRenderer};
