//! # Errors during interactive picker usage
//! This module contains the custom error type [`PickError`] returned by the
//! [`Picker::pick`](crate::Picker::pick) method, and siblings `Picker::pick_*`. The error type is
//! comprehensive and the individual picker method may or may not result in the corresponding
//! errors.
//!
//! See the [`PickError`] documentation for more detail.
//!
//! ## Example
//! Convert a [`PickError::Aborted`] silently into no choice, propogating any other error as an IO
//! error. Use with `picker.pick().or_else(suppress_abort)`.
//! ```
//! # use nucleo_picker::error::PickError;
//! # use std::io;
//! fn suppress_abort<D: Default>(err: PickError) -> Result<D, io::Error> {
//!     match err {
//!         PickError::Aborted => Ok(D::default()),
//!         e => Err(e.into()),
//!     }
//! }
//!
//! ```

use std::io;

/// An error which may be returned while running the picker interactively.
///
/// The error type is (in spirit) an [`io::Error`], but with more precise variants not present in
/// the default [`io::Error`]. For convenience, there is a `From<PickError> for io::Error`
/// implementation; this propogates the underlying IO error and converts any other error message to
/// an [`io::Error`] using [`io::Error::other`].
///
/// This is marked non-exhaustive since more variants may be added in the future. It is recommended
/// to handle the errors that are relevant to your application and propogate any remaining errors
/// as an [`io::Error`].
#[derive(Debug)]
#[non_exhaustive]
pub enum PickError {
    /// A read or write resulted in an IO error.
    IO(io::Error),
    /// The event stream disconnected while the picker was still running.
    Disconnected,
    /// The picker was aborted.
    Aborted,
    /// The picker could not be started since the writer is not interactive.
    NotInteractive,
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::IO(error) => error.fmt(f),
            PickError::Disconnected => {
                f.write_str("event source disconnected while picker was still active")
            }
            PickError::Aborted => f.write_str("picker received abort event"),
            PickError::NotInteractive => {
                f.write_str("picker could not start since the screen is not interactive")
            }
        }
    }
}

impl std::error::Error for PickError {}

impl From<io::Error> for PickError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

impl From<PickError> for io::Error {
    fn from(err: PickError) -> Self {
        match err {
            PickError::IO(io_error) => io_error,
            PickError::Disconnected => {
                io::Error::other("event source disconnected while picker was still active")
            }
            PickError::Aborted => io::Error::other("received abort event"),
            PickError::NotInteractive => io::Error::other("writer is not interactive"),
        }
    }
}
