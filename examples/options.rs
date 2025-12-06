//! # Demonstration of configuration options
//!
//! This blocking example demonstrates some of the configuration options available to the picker.
use std::io::Result;

use nucleo_picker::{CaseMatching, PickerOptions, render::StrRenderer};

fn main() -> Result<()> {
    let mut picker = PickerOptions::default()
        // set the configuration to match 'path-like' objects
        .match_paths()
        // ignore case when matching
        .case_matching(CaseMatching::Ignore)
        // set the default prompt to `/var`
        .query("/var")
        .picker(StrRenderer);

    let choices = vec![
        "/var/tmp/a",
        "/var/tmp/b",
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
        injector.push(opt);
    }

    // open interactive prompt
    match picker.pick()? {
        Some(opt) => println!("You selected: '{opt}'"),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
