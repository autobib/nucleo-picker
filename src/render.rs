//! # Renderers for use in a [`Picker`](super::Picker)
//!
//! This module contains built-in renderers to be used in a [`Picker`](super::Picker), and (with
//! appropriate types) can be used as the arguments passed to the
//! [`PickerOptions::picker`](super::PickerOptions::picker) and [`Picker::new`](super::Picker::new)
//! methods.
use std::{borrow::Cow, path::Path};

use super::Render;

/// A renderer for any type which de-references as [`str`], such as a [`String`].
pub struct StrRenderer;

impl<T: AsRef<str>> Render<T> for StrRenderer {
    type Str<'a>
        = &'a str
    where
        T: 'a;

    fn render<'a>(&self, value: &'a T) -> Self::Str<'a> {
        value.as_ref()
    }
}

/// A renderer for any type which de-references as [`Path`], such as a
/// [`PathBuf`](std::path::PathBuf).
pub struct PathRenderer;

impl<T: AsRef<Path>> Render<T> for PathRenderer {
    type Str<'a>
        = Cow<'a, str>
    where
        T: 'a;

    fn render<'a>(&self, value: &'a T) -> Self::Str<'a> {
        value.as_ref().to_string_lossy()
    }
}

/// A renderer which uses a type's [`Display`](std::fmt::Display) implementation.
pub struct DisplayRenderer;

impl<T: ToString> Render<T> for DisplayRenderer {
    type Str<'a>
        = String
    where
        T: 'a;

    fn render<'a>(&self, value: &'a T) -> Self::Str<'a> {
        value.to_string()
    }
}
