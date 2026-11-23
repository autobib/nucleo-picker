//! # Interactive interface
//!
//! This is a crate implementing picker interfaces for use in the high-level
//! [nucleo-picker](https://docs.rs/nucleo-picker) crate or in the low-level [nucleo-picker-core](https://docs.rs/nucleo-picker-core) crate.
//! You probably want to start there, instead of reading the documentation here first.
//!
//! In short, the *picker interface* describes the behaviour of the picker during interactive operation.
//! The [`Interface`] governs the interactive behaviour of the picker (so that
//! you only pay for interface machinery you actually use) as well as the final return type.
//!
//! This crate implements built-in interfaces:
//!
//! - A minimal [`Single`] interface, for single-selections with no preview.
//! - A [`Multi`] interface, for multiple-selections with no preview.
//! - A [`UncachedPreview`] interface wrapper, which adds a basic preview interface to any internal
//!   interface.
//! - A [`CachedPreview`] interface wrapper, which adds a cache-based preview interface (requires
//!   the `preview-cache` feature).
//!
//! ## Previewers
//!
//! The [`CachedPreview`] interface wrapper depends on the [`Preview`](preview::Preview) trait to
//! coordinate preview requests in a way that is compatible both with a background threadpool and with synchronous requests.
//! It is the most robust and recommended way to add a previewer to an interface. In particular, see
//! the [`PoolPreviewer`](preview::PoolPreviewer) for a convenient threadpool implementation which
//! can be immediately used with the [`CachedPreview`] interface.
//!
//! Visit the [`preview`] module for more details.
//!
//! ## Cargo features
//!
//! This library uses Cargo features to enable and disable various behaviour, especially when that
//! behaviour would require additional dependencies to operate.
//! The features are as follows:
//!
//! | Feature | Default | Dependencies | Description |
//! | --- | --- | --- | --- |
//! | `preview-cache` | No | [`lru`], [`parking_lot`] | This enables the [`Preview`](preview::Preview) trait and [`CachedPreview`]. |

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "preview-cache")]
mod cached;
mod multi;
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub mod preview;
mod selection;
mod single;
#[cfg(test)]
mod test_support;
mod uncached;

#[cfg(feature = "preview-cache")]
pub use cached::{CachedPreview, CachedPreviewState};
pub use multi::{Multi, MultiState};
pub use nucleo_picker_core::interface::Interface;
pub use selection::{Selection, SelectionIntoIter};
pub use single::Single;
pub use uncached::{UncachedPreview, UncachedPreviewState};
