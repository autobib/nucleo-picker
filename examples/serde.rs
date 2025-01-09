//! # Serde support and multiline rendering
//!
//! This example demonstrates how to use serde support when rendering from an input sequence. The
//! example also incorporates multi-line items to demonstrate large item rendering.
//!
//! This example requires the `serde` feature: run with
//! ```bash
//! cargo run --release --example serde --features serde
//! ```
use std::{io::Result, thread::spawn};

use nucleo_picker::{Picker, Render};
use serde::{de::DeserializeSeed, Deserialize};
use serde_json::Deserializer;

/// The picker item, which also implements [`Deserialize`].
#[derive(Deserialize)]
struct Poem {
    author: String,
    title: String,
    lines: Vec<String>,
}

struct PoemRenderer;

impl Render<Poem> for PoemRenderer {
    type Str<'a> = String;

    /// Render the text of the poem by joining the lines.
    fn render<'a>(&self, poem: &'a Poem) -> Self::Str<'a> {
        poem.lines.join("\n")
    }
}

fn main() -> Result<()> {
    let mut picker = Picker::new(PoemRenderer);
    let injector = picker.injector();

    spawn(move || {
        // just for the example; usually you would read this from a file at runtime or similar and
        // instead use `serde_json::from_reader`.
        let poems_json = include_str!("poems.json");

        // use the deserialize implementation of a `Poem` to deserialize from the contents of
        // `poems.json`. the `DeserializeSeed` implementation of `&Injector` expects that the input
        // is a sequence of values which can be deserialized into the picker item, which in this
        // case is a `Poem`.
        injector
            .deserialize(&mut Deserializer::from_str(poems_json))
            .unwrap();
    });

    // open interactive prompt
    match picker.pick()? {
        Some(poem) => println!("'{}' by {}", poem.title, poem.author),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
