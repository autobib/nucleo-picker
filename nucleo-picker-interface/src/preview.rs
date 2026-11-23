//! # Render item previews in the picker
//!
//! This module contains the core [`Preview`] trait and associated types. A *previewer* is a type
//! which knows how to generate previews for the item `T` in the picker. The preview is rendered
//! in the *preview pane*, which is a special window rendered beside the active matches.
//!
//! In many cases, you can avoid the full details of implementing a previewer:
//!
//! - For fast non-blocking previews, see [`SyncPreviewer`].
//! - For slow previews that should be performed in a background thread pool, see [`PoolPreviewer`].
//!
//! If you are doing something more complicated, start at the [`Preview`] documentation and then
//! proceed with the [`request`] module. Here are some other useful resources:
//!
//! - There are full implementation examples in the [examples folder](https://github.com/autobib/nucleo-picker/tree/master/examples)
//!   on GitHub.
//! - Default keybindings for preview events can be found in the [picker usage documentation](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md)
//! - For documentation concerning the internal preview cache, see the [preview interface
//!   docs](crate::CachedPreview#preview-cache).

mod cache;
mod controller;
mod lock;
mod pool;
mod provider;
pub mod request;

pub(crate) use controller::PreviewState;
pub use pool::{PoolPreviewer, Worker};
pub use provider::{Preview, SyncPreviewer};
