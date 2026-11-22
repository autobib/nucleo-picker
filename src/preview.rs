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
//! - Default keybindings for preview events can be found in the [picker usage documetation](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md)
//! - For documentation concerning the internal preview cache, see the [preview interface
//!   docs](CachedPreview#preview-cache).
//! - There is a [basic example](#basic-example), [extended example](#extended-example), and a
//!   [simple background preview example](#background-preview-example).
//!
//! ## Using previews
//!
//! In short, start with a [`Picker`](crate::Picker) and add a previewer using
//! [`Picker::with_preview`](crate::Picker::with_preview). The type transitions are compatible with
//! [`Picker::multi`](crate::Picker::multi) for previewing pickers with multiple selections.
//!
//! For one-off usage, the above methods are typically sufficient. Note that if `P` implements
//! [`Preview`], then `&mut P` also implements [`Preview`]: this can be useful if the previewer has
//! state which should be preserved between calls or which should be accessed after selection is
//! complete.
//!
//! ### Fatal errors
//!
//! For fatal errors, it is recommended to pass a handle to the
//! [`EventSource`](crate::event::EventSource) and propagate errors directly to the picker using
//! [`Event::Abort`](crate::event::Event::Abort). For instance, if using a channel with
//! [`StdinEventSender`](crate::event::StdinEventSender), one can pass senders to the
//! previewer or preview workers directly.
//!
//! See the [online preview example](https://github.com/autobib/nucleo-picker/blob/master/examples/online_preview.rs)
//! for a complete example highlighting propagation of fatal preview errors.
//!
//! ## Examples
//!
//! ### Basic example
//!
//! Here is an example highlighting basic usage.
//!
//! ```no_run
//! # fn main() -> std::io::Result<()> {
//! # #[cfg(feature = "preview-cache")]
//! # {
//! use nucleo_picker::{
//!     Picker,
//!     interface::Multi,
//!     preview::{Pane, SyncPreviewer},
//!     render::StrRenderer,
//! };
//!
//! let picker = Picker::new(StrRenderer);
//! picker.push_batch(["red", "blue", "green"]);
//!
//! // this is a bit of a silly previewer, which just previews the item
//! // text itself, but it's enough for the example
//! let previewer = SyncPreviewer(|color: &&str, pane: &mut Pane| {
//!     pane.buffer_mut().push_text(color);
//! });
//!
//! // run the picker with previews
//! let mut picker = picker.with_preview(previewer);
//! if let Some(color) = picker.pick()? {
//!     println!("{color}");
//! }
//!
//! // run the picker without previews and with multiple selections
//! // without consuming the original picker or previewer
//! for color in picker.as_interface(Multi::new()).pick()? {
//!     println!("{color}");
//! }
//! # }
//! # Ok(())
//! # }
//! ```
//!
//! ### Synchronous preview example
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
//! use std::borrow::Cow;
//! use nucleo_picker::{Picker, preview::{Pane, SyncPreviewer}};
//!
//! struct Item {
//!     name: &'static str,
//!     description: &'static str,
//! }
//!
//! fn render(item: &Item) -> Cow<'_, str> {
//!     Cow::Borrowed(item.name)
//! }
//!
//! # fn main() -> std::io::Result<()> {
//! let picker = Picker::new(render);
//! picker.push_batch([
//!     Item { name: "red", description: "RGB: 255, 0, 0" },
//!     Item { name: "blue", description: "RGB: 0, 0, 255" },
//! ]);
//! let previewer = SyncPreviewer(|item: &Item, pane: &mut Pane| {
//!     pane.buffer_mut().push_text(item.description);
//! });
//! let mut picker = picker.with_preview(previewer);
//! if let Some(item) = picker.pick()? {
//!     println!("{}", item.name);
//! }
//! # Ok(())
//! # }
//! # }
//! # fn main() {}
//! ```
//!
//! ### Thread-pool preview example
//!
//! Here is a basic example implementing a file previewer.
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
//! use std::{fs, num::NonZeroUsize, path::PathBuf};
//! use nucleo_picker::{
//!     Picker,
//!     preview::{Pane, PoolPreviewer, Worker},
//!     render::PathRenderer,
//! };
//!
//! #[derive(Clone)]
//! struct FilePreview;
//!
//! impl Worker<PathBuf> for FilePreview {
//!     fn preview(
//!         &mut self,
//!         path: &PathBuf,
//!         pane: &mut Pane,
//!         _is_cancelled: impl Fn() -> bool,
//!     ) -> bool {
//!         match fs::read_to_string(path) {
//!             Ok(text) => pane.push_text(&text),
//!             Err(error) => {
//!                 pane.set_err(true);
//!                 pane.push_text(&error.to_string());
//!             }
//!         }
//!         true
//!     }
//! }
//!
//! # fn main() -> std::io::Result<()> {
//! let picker = Picker::new(PathRenderer);
//! picker.push_batch([PathBuf::from("Cargo.toml"), PathBuf::from("README.md")]);
//! let previewer = PoolPreviewer::from_worker(FilePreview, NonZeroUsize::new(2).unwrap())?;
//! let mut picker = picker.with_preview(previewer);
//! if let Some(path) = picker.pick()? {
//!     println!("{}", path.display());
//! }
//! # Ok(())
//! # }
//! # }
//! # fn main() {}
//! ```
//!
//! ### Extended example
//!
//! This example uses a stateful previewer. Run this example with
//! `cargo run --release --example preview_style --features preview-cache`.
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
#![doc = include_str!("../examples/preview_style.rs")]
//! # }
//! # fn main() {}
//! ```
//!
//! ### Background preview example
//!
//! Here is a basic example of a previewer running in a background thread. This also uses the
//! [`Worker`] trait, which is the same abstraction used by a [`PoolPreviewer`].
//!
//! ```no_run
//! # #[cfg(feature = "preview-cache")]
//! # mod example {
#![doc = include_str!("../examples/preview_single.rs")]
//! # }
//! # fn main() {}
//! ```

#[cfg(feature = "preview-ansi")]
pub use nucleo_picker_core::preview::{AnsiState, AnsiWriter};
pub use nucleo_picker_core::preview::{
    BoundaryChars, Buffer, Content, Line, Pane, TextWriter, Viewport,
};
#[cfg(feature = "preview-cache")]
#[doc(inline)]
pub use nucleo_picker_interface::CachedPreview;
#[doc(inline)]
pub use nucleo_picker_interface::UncachedPreview;
/// There are implementation examples in the [module-level docs](crate::preview#examples).
#[cfg(feature = "preview-cache")]
#[doc(inline)]
pub use nucleo_picker_interface::preview::Preview;
#[cfg(feature = "preview-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview-cache")))]
pub use nucleo_picker_interface::preview::request;
#[cfg(feature = "preview-cache")]
pub use nucleo_picker_interface::preview::{PoolPreviewer, SyncPreviewer, Worker};
