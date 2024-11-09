//! # Basic blocking picker
//!
//! This is almost a minimal example, but not really a good example of what to do in practice unless
//! the number of items is very small since we block the main thread to populate the matcher. See
//! [`find`](/examples/find.rs) for a (somewhat) more realistic use-case.
use std::io::Result;

use nucleo_picker::{render::StrRender, Picker};

fn main() -> Result<()> {
    let mut picker = Picker::new(StrRender);

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
    let mut injector = picker.injector();
    for opt in choices {
        // Use `RenderStr` renderer to generate the match contents, since the choices are already
        // string types.
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
