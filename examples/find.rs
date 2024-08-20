use std::{env::args, io, path::PathBuf, thread};
use walkdir::{DirEntry, WalkDir};

use nucleo_picker::{
    nucleo::{Config, Injector},
    Picker,
};

fn main() -> Result<(), io::Error> {
    let config = Config::DEFAULT.match_paths();
    let mut picker = Picker::new_with_config(1, config);

    let injector: Injector<DirEntry> = picker.injector();

    let root: PathBuf = match args().nth(1) {
        Some(path) => path.into(),
        None => ".".into(),
    };

    // populate from a separate thread to avoid locking the picker interface
    thread::spawn(move || {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            // how to we represent the item for the picker?
            let picker_display = entry.path().display().to_string().into();

            // the picker only has one column
            let _ = injector.push(entry, move |_, cols| {
                cols[0] = picker_display;
            });
        }
    });

    match picker.pick()? {
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
