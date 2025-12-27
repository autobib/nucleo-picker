//! # Demonstration of configuration options, some of which are un-renderable, demonstrating the filtering ability
//!
//! This blocking example demonstrates some of the configuration options available to the picker.
use std::io::Result;
use std::ops::RangeInclusive;

use nucleo_picker::{nucleo::Config, render::StrRenderer, PickerOptions};

const ASCII_CONTROL_CHARS: RangeInclusive<char> = '\x00'..='\x1F';

fn main() -> Result<()> {
    let mut picker = PickerOptions::default()
        // set the configuration to match 'path-like' objects
        .config(Config::DEFAULT.match_paths())
        // set the default query string to `/var`
        .query("/var")
        .picker(StrRenderer);

    // populate the matcher
    let injector = picker.injector();
    for ctrl in ASCII_CONTROL_CHARS {
        let hex_ctrl = format!("0x{:X}", ctrl as u8); // Convert the char to a u32 and format as hex
        let option = format!("hex is {} rendered is {} EOL", hex_ctrl, ctrl);
        injector.push(option);
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
