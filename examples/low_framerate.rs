//! The `blocking` example, but with an extremely low frame-rate (0.5 FPS)
use std::{io, time::Duration};

use nucleo_picker::{PickerOptions, render::StrRenderer};

fn main() -> io::Result<()> {
    let mut picker = PickerOptions::new()
        .frame_interval(Duration::from_secs(2))
        .picker(StrRenderer);

    let choices = vec![
        "Rembrandt",
        "Velázquez",
        "Schiele",
        "Hockney",
        "Klimt",
        "Bruegel",
        "Magritte",
        "Carvaggio",
    ];

    // populate the matcher
    let injector = picker.injector();
    for opt in choices {
        // Use `RenderStr` renderer to generate the match contents, since the choices are already
        // string types.
        injector.push(opt);
    }

    // open interactive prompt
    match picker.pick()? {
        Some(opt) => println!("You selected: '{opt}'"),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
