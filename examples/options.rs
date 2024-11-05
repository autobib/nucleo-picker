//! # Demonstration of configuration options
//!
//! This blocking example demonstrates some of the configuration examples available to the picker.
use std::io::Result;

use nucleo_picker::{nucleo::Config, PickerOptions};

fn main() -> Result<()> {
    let mut opts = PickerOptions::default();
    opts
        // set the configuration to match 'path-like' objects
        .config(Config::DEFAULT.match_paths())
        // set the default query string to `/var`
        .query("/var");

    let mut picker = opts.picker();

    let choices = vec![
        "/var/tmp",
        "/var",
        "/usr/local",
        "/usr",
        "/usr/local/share",
        "/dev",
    ];

    // populate the matcher
    let injector = picker.injector();
    for opt in choices {
        let _ = injector.push(opt, |e, cols| cols[0] = e.to_owned().into());
    }

    // open interactive prompt
    match picker.pick()? {
        Some(opt) => {
            println!("You selected: '{opt}'");
        }
        None => {
            println!("Nothing selected!");
        }
    }

    Ok(())
}
