//! # Renderers for use in a [`Picker`](super::Picker)
//!
//! This module contains built-in renderers to be used in a [`Picker`](super::Picker), and (with
//! appropriate types) can be used as the arguments passed to the
//! [`PickerOptions::picker`](super::PickerOptions::picker) and [`Picker::new`](super::Picker::new)
//! methods.
use std::{borrow::Cow, path::Path};

use super::Render;

/// A renderer for any type which de-references as [`str`], such as a [`String`].
#[derive(Copy, Clone)]
pub struct StrRender;

impl<T: AsRef<str>> Render<T> for StrRender {
    type Column<'a>
        = &'a str
    where
        T: 'a;

    fn as_column<'a>(&'a mut self, value: &'a T) -> Self::Column<'a> {
        value.as_ref()
    }
}

/// A renderer for any type which de-references as [`Path`], such as a
/// [`PathBuf`](std::path::PathBuf).
#[derive(Copy, Clone)]
pub struct PathRender;

impl<T: AsRef<Path>> Render<T> for PathRender {
    type Column<'a>
        = Cow<'a, str>
    where
        T: 'a;

    fn as_column<'a>(&'a mut self, value: &'a T) -> Self::Column<'a> {
        value.as_ref().to_string_lossy()
    }
}

/// A renderer which uses a type's [`Display`](std::fmt::Display) implementation.
#[derive(Copy, Clone)]
pub struct DisplayRender;

impl<T: ToString> Render<T> for DisplayRender {
    type Column<'a>
        = String
    where
        T: 'a;

    fn as_column<'a>(&'a mut self, value: &'a T) -> Self::Column<'a> {
        value.to_string()
    }
}
