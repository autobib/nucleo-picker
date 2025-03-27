//! # Non-blocking `find`-style picker
//!
//! Iterate over directories to populate the picker, but do not block so that
//! matching can be done while the picker is populated.
use std::{borrow::Cow, env::args, io, path::PathBuf, thread::spawn};

use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{nucleo::Config, PickerOptions, Render};

pub struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Str<'a> = Cow<'a, str>;

    /// Render a `DirEntry` using its internal path buffer.
    fn render<'a>(&self, value: &'a DirEntry) -> Self::Str<'a> {
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
    let injector = picker.injector();
    spawn(move || {
        WalkBuilder::new(root).build_parallel().run(|| {
            let injector = injector.clone();
            Box::new(move |walk_res| {
                if let Ok(dir) = walk_res {
                    injector.push(dir);
                }
                WalkState::Continue
            })
        });
    });

    match picker.pick()? {
        // the matched `entry` is `&DirEntry`
        Some(entry) => println!("Path of selected file: '{}'", entry.path().display()),
        None => println!("No file selected!"),
    }

    Ok(())
}
