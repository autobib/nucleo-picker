//! # Basic blocking picker
//!
//! This is almost a minimal example, but not really a good example of what to do in practice unless
//! the number of items is very small since we block the main thread to populate the matcher. See
//! [`find`](/examples/find.rs) for a (somewhat) more realistic use-case.
use std::io::Result;

use nucleo_picker::{
    nucleo::Utf32String, // nucleo re-export
    Picker,
};

/// The item type that we are picking. This can be anything that is Send + Sync + 'static.
type Item = &'static str;

/// Format the item for display within the `Nucleo` instance.
fn set_nucleo_column(i: &Item, cols: &mut [Utf32String]) {
    // only set column 0 since the picker (by default) has 1 column
    cols[0] = (i as &str).into();
}

fn main() -> Result<()> {
    let mut picker = Picker::default();

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
    let injector = picker.injector(); // this is just a `nucleo::Injector`;
    for opt in choices {
        let _ = injector.push(opt, set_nucleo_column);
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
