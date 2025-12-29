//! # Multiline example
//!
//! This is identical to the 'blocking' example, but allowing multiple picked items.
use std::io;

use nucleo_picker::{Picker, render::StrRenderer};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

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
    for it in picker.pick_multi()?.iter() {
        println!("{it}");
    }

    Ok(())
}
