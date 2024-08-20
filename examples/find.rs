//! # Non-blocking `find`-style picker
//!
//! Iterate over directories to populate the picker, but do not block so that matching can be done
//! while the picker is populated.
use std::{env::args, io, path::PathBuf, thread};
use walkdir::WalkDir;

use nucleo_picker::{nucleo::Config, Picker};

fn main() -> io::Result<()> {
    // See the nucleo configuration for more options:
    //   https://docs.rs/nucleo/latest/nucleo/struct.Config.html
    let config = Config::DEFAULT.match_paths();

    // Initialize a picker with 1 column and the provided configuration
    //   NOTE: multi-column is not currently supported
    let mut picker = Picker::new_with_config(1, config);

    let injector = picker.injector();

    // "argument parsing"
    let root: PathBuf = match args().nth(1) {
        Some(path) => path.into(),
        None => ".".into(),
    };

    // populate from a separate thread to avoid locking the picker interface
    thread::spawn(move || {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let _ = injector.push(entry, move |e, cols| {
                // the picker only has one column; fill it with the match text
                cols[0] = e.path().display().to_string().into();
            });
        }
    });

    match picker.pick()? {
        // the matched `entry` is &DirEntry
        Some(entry) => {
            println!(
                "Name of selected file: '{}'",
                entry.file_name().to_string_lossy()
            );
        }
        None => {
            println!("No file selected!");
        }
    }

    Ok(())
}
