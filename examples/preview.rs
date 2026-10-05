//! A minimal preview example.
//!
//! ```bash
//! cargo run --release --example preview --features preview
//! ```
//!
//! All data is from Wikipedia.
use std::{borrow::Cow, io};

use crossterm::style::{ContentStyle, Stylize};
use nucleo_picker::{
    Picker,
    preview::{PreviewBuffer, SyncPreviewer},
};

struct Artist {
    name: &'static str,
    born: &'static str,
    birthplace: &'static str,
    movement: &'static str,
}

fn render_artist(artist: &Artist) -> Cow<'_, str> {
    artist.name.into()
}

fn main() -> io::Result<()> {
    let mut picker = Picker::new(render_artist);
    picker.push_batch([
        Artist {
            name: "Rembrandt",
            born: "15 July 1606",
            birthplace: "Leiden",
            movement: "Baroque",
        },
        Artist {
            name: "Gustav Klimt",
            born: "14 July 1862",
            birthplace: "Baumgarten (near Vienna)",
            movement: "Symbolism",
        },
        Artist {
            name: "René Magritte",
            born: "21 November 1898",
            birthplace: "Lessines",
            movement: "Surrealism",
        },
    ]);

    let previewer = SyncPreviewer(|artist: &Artist, buffer: &mut PreviewBuffer| {
        buffer.push_styled_line(artist.name, ContentStyle::new().bold());
        for (label, value) in [
            ("Born:       ", artist.born),
            ("Birthplace: ", artist.birthplace),
            ("Movement:   ", artist.movement),
        ] {
            buffer.newline();
            buffer.push_styled_str(label, ContentStyle::new().cyan().bold());
            buffer.push_str(value);
        }
    });

    match picker.with_preview(previewer).pick()? {
        Some(artist) => println!("You selected: '{}'", artist.name),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
