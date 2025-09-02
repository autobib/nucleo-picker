//! # Serde support and multiline rendering
//!
//! This example demonstrates how to use serde support when rendering from an input sequence. The
//! example also incorporates multi-line items to demonstrate large item rendering.
//!
//! This example requires the `serde` feature: run with
//! ```bash
//! cargo run --release --example serde --features serde
//! ```
//! To try out the 'reversed' rendering, add the '--reversed' command line option:
//! ```bash
//! cargo run --release --example serde --features serde -- --reversed
//! ```
use std::{env::args, io::Result, thread::spawn};

use nucleo_picker::{PickerOptions, Render};
use serde::{Deserialize, de::DeserializeSeed};
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
    // "argument parsing"
    let opts = PickerOptions::new();
    let picker_opts = match args().nth(1) {
        Some(s) if s == "--reversed" => opts.reversed(true),
        _ => opts,
    };

    let mut picker = picker_opts.picker(PoemRenderer);
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
