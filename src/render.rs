//! # Renderers for use in a [`Picker`](super::Picker)
//!
//! This module contains built-in renderers to be used in a [`Picker`](super::Picker), and (with
//! appropriate types) can be used as the arguments passed to the
//! [`PickerOptions::picker`](super::PickerOptions::picker) and [`Picker::new`](super::Picker::new)
//! methods.
use std::{borrow::Cow, path::Path};

use super::Render;

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
pub struct StrRenderer;

impl<T: AsRef<str>> Render<T> for StrRenderer {
    type Str<'a>
        = &'a str
    where
        T: 'a;

    fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
        item.as_ref()
    }
}

/// A renderer for any type which de-references as [`Path`], such as a
/// [`PathBuf`](std::path::PathBuf).
///
/// ## Example
/// ```
/// # use nucleo_picker::{render::PathRenderer, Render};
/// use std::path::PathBuf;
/// let path_renderer = PathRenderer;
///
/// let mut path = PathBuf::new();
///
/// path.push("/");
/// path.push("dev");
/// path.push("null");
///
/// // Note: platform-dependent output
/// assert_eq!(path_renderer.render(&path), "/dev/null");
/// ```
pub struct PathRenderer;

impl<T: AsRef<Path>> Render<T> for PathRenderer {
    type Str<'a>
        = Cow<'a, str>
    where
        T: 'a;

    fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
        item.as_ref().to_string_lossy()
    }
}

/// A renderer which uses a type's [`Display`](std::fmt::Display) implementation.
///
/// ## Example
/// ```
/// # use nucleo_picker::{render::DisplayRenderer, Render};
/// let display_renderer = DisplayRenderer;
///
/// assert_eq!(display_renderer.render(&1.624f32), "1.624");
/// ```
pub struct DisplayRenderer;

impl<T: ToString> Render<T> for DisplayRenderer {
    type Str<'a>
        = String
    where
        T: 'a;

    fn render<'a>(&self, item: &'a T) -> Self::Str<'a> {
        item.to_string()
    }
}
