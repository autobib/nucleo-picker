//! # Errors during interactive picker usage
//! This module contains the custom error type [`PickError`] returned by the
//! [`Picker::pick`](crate::Picker::pick) method, and siblings `Picker::pick_*`. The error type is
//! comprehensive and the individual picker method used may or may not result in the corresponding
//! errors.
//!
//! See the [`PickError`] documentation for more detail.
//!
//! ## Example
//! Convert a [`PickError::UserInterrupted`] silently into no choice, propogating any other error as an IO
//! error. Use with `picker.pick().or_else(suppress_abort)`.
//! ```
//! # use nucleo_picker::error::PickError;
//! # use std::io;
//! fn suppress_abort<A, D>(err: PickError<A>) -> Result<D, io::Error>
//! where
//!     A: std::error::Error + Send + Sync + 'static,
//!     D: Default,
//! {
//!     match err {
//!         PickError::UserInterrupted => Ok(D::default()),
//!         e => Err(e.into()),
//!     }
//! }
//! ```

use std::{convert::Infallible, error::Error as StdError, io};

/// An error which may be returned while running the picker interactively.
///
/// The error type is (in spirit) an [`io::Error`], but with more precise variants not present in
/// the default [`io::Error`]. For convenience and (partial) backwards compatibility, there is a
/// `From<PickError> for io::Error` implementation; this propogates the underlying IO error and
/// converts any other error message to an [`io::Error`] using [`io::Error::other`].
///
/// This is marked non-exhaustive since more variants may be added in the future. It is recommended
/// to handle the errors that are relevant to your application and propogate any remaining errors
/// as an [`io::Error`].
///
/// ## Type paremter for `Aborted` variant
/// The [`PickError::Aborted`] variant can be used by the application to propagate errors to the
/// picker; the application-defined error type is the type parameter `A`. By default, `A = !`
/// which means this type of abort will *never occur* and can be ignored during pattern matching.
///
/// This library will never generate an abort error directly. In order to pass errors downstream to
/// the picker, the application can define an abort error type using the
/// [`EventSource::AbortErr`](crate::EventSource::AbortErr) associated type. This associated type
/// is the same as the type parameter here when used in
/// [`Picker::pick_with_io`](crate::Picker::pick_with_io).
#[derive(Debug)]
#[non_exhaustive]
pub enum PickError<A = Infallible> {
    /// A read or write resulted in an IO error.
    IO(io::Error),
    /// The event stream disconnected while the picker was still running.
    Disconnected,
    /// The picker quit at the user's request.
    UserInterrupted,
    /// The picker could not be started since the writer is not interactive.
    NotInteractive,
    /// The picker was aborted because of an upstream error.
    Aborted(A),
}

impl<A: StdError> std::fmt::Display for PickError<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::IO(error) => error.fmt(f),
            PickError::Disconnected => {
                f.write_str("event source disconnected while picker was still active")
            }
            PickError::Aborted(err) => write!(f, "received abort: {err}"),
            PickError::UserInterrupted => f.write_str("keyboard interrupt"),
            PickError::NotInteractive => {
                f.write_str("picker could not start since the screen is not interactive")
            }
        }
    }
}

impl<A: StdError> StdError for PickError<A> {}

impl<A: StdError> From<io::Error> for PickError<A> {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

impl<A: StdError + Send + Sync + 'static> From<PickError<A>> for io::Error {
    fn from(err: PickError<A>) -> Self {
        match err {
            PickError::IO(io_error) => io_error,
            PickError::Disconnected => {
                io::Error::other("event source disconnected while picker was still active")
            }
            PickError::UserInterrupted => io::Error::other("keyboard interrupt"),
            PickError::Aborted(err) => io::Error::other(err),
            PickError::NotInteractive => io::Error::other("writer is not interactive"),
        }
    }
}
