//! # Non-blocking `find`-style picker
//!
//! Iterate over directories to populate the picker, but do not block so that
//! matching can be done while the picker is populated.
use std::{borrow::Cow, env::args, io, path::PathBuf, process::exit, thread::spawn};

use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{PickerOptions, Render};

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
        // Optimize scoring algorithm for paths.
        .match_paths()
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
        // add items to the picker from many threads in parallel
        WalkBuilder::new(root).build_parallel().run(|| {
            let injector = injector.clone(); // this is very cheap (`Arc::clone`)
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
        Some(entry) => println!("{}", entry.path().display()),
        None => {
            eprintln!("No path selected!");
            exit(1);
        }
    }

    Ok(())
}
