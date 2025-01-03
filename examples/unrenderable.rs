//! # Demonstration of configuration options, some of which are un-renderable, demonstrating the filtering ability 
//!
//! This blocking example demonstrates some of the configuration options available to the picker.
use std::io::Result;

use nucleo_picker::{nucleo::Config, render::StrRenderer, PickerOptions};


fn list_control_characters() -> Vec<char> {
    (0x00..=0x1F).map(|i| char::from_u32(i).unwrap()).collect()
}
fn main() -> Result<()> {
    let mut picker = PickerOptions::default()
        // set the configuration to match 'path-like' objects
        .config(Config::DEFAULT.match_paths())
        // set the default query string to `/var`
        .query("/var")
        .picker(StrRenderer);


        
        
    let control_chars = list_control_characters();
            

    // populate the matcher
    let injector = picker.injector();
    for ctrl in control_chars {
        let hex_ctrl = format!("{:X}", ctrl as u32);  // Convert the char to a u32 and format as hex
        let option = format!("hex is {} rendered is {} EOL",hex_ctrl, ctrl);
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