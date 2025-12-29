//! # Multiline example
//!
//! This is identical to the 'blocking' example but allowing multiple picked items.
use std::{io, num::NonZero};

use nucleo_picker::{PickerOptions, render::StrRenderer};

fn main() -> io::Result<()> {
    let mut picker = PickerOptions::new()
        // allow at most 3 selections
        .max_selection_count(NonZero::new(3))
        .picker(StrRenderer);

    picker.extend_exact([
        "Rembrandt",
        "Velázquez",
        "Schiele",
        "Hockney",
        "Klimt",
        "Bruegel",
        "Magritte",
        "Carvaggio",
    ]);

    // open interactive prompt, and do not return an error if there is no selection
    for it in picker.pick_multi()?.iter() {
        println!("{it}");
    }

    Ok(())
}
