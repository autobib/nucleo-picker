//! # Demonstration of configuration options
//!
//! This blocking example demonstrates some of the configuration examples available to the picker.
use std::io::Result;

use nucleo_picker::{nucleo::Config, render::StrRender, PickerOptions};

fn main() -> Result<()> {
    let mut picker = PickerOptions::default()
        // set the configuration to match 'path-like' objects
        .config(Config::DEFAULT.match_paths())
        // set the default query string to `/var`
        .query("/var")
        .picker(StrRender);

    let choices = vec![
        "/var/tmp",
        "/var",
        "/usr/local",
        "/usr",
        "/usr/local/share",
        "/dev",
    ];

    // populate the matcher
    let mut injector = picker.injector();
    for opt in choices {
        injector.push(opt);
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
