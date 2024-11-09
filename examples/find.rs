//! # Non-blocking `find`-style picker
//!
//! Iterate over directories to populate the picker, but do not block so that
//! matching can be done while the picker is populated.
use std::{borrow::Cow, env::args, io, path::PathBuf, thread::spawn};
use walkdir::{DirEntry, WalkDir};

use nucleo_picker::{nucleo::Config, PickerOptions, Render};

#[derive(Clone)]
pub struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Column<'a> = Cow<'a, str>;

    // Render a `DirEntry` using its internal path buffer.
    fn as_column<'a>(&mut self, value: &'a DirEntry) -> Self::Column<'a> {
        value.path().to_string_lossy()
    }
}

fn main() -> io::Result<()> {
    let mut picker = PickerOptions::default()
        // See the nucleo configuration for more options:
        //   https://docs.rs/nucleo/latest/nucleo/struct.Config.html
        .config(Config::DEFAULT.match_paths())
        // Use our custom renderer for a `DirEntry`
        .picker(DirEntryRender);

    // "argument parsing"
    let root: PathBuf = match args().nth(1) {
        Some(path) => path.into(),
        None => ".".into(),
    };

    // populate from a separate thread to avoid locking the picker interface
    let mut injector = picker.injector();
    spawn(move || {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            injector.push(entry);
        }
    });

    match picker.pick()? {
        Some(entry) => {
            // the matched `entry` is `&DirEntry`
            println!("Path of selected file: '{}'", entry.path().display());
        }
        None => {
            println!("No file selected!");
        }
    }

    Ok(())
}
