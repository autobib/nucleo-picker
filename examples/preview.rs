//! # Basic preview example
//!
//! This is a basic synchronous preview example. Run with
//!
//! ```bash
//! cargo run --release --example preview --features serde,preview
//! ```
use std::{io, thread::spawn};

use nucleo_picker::{
    PickerOptions, Render,
    preview::{PreviewBuffer, SyncPreviewer},
};
use serde::{Deserialize, de::DeserializeSeed};
use serde_json::Deserializer;

#[derive(Deserialize)]
struct Poem {
    author: String,
    title: String,
    lines: Vec<String>,
}

struct PoemRenderer;

impl Render<Poem> for PoemRenderer {
    type Str<'a> = &'a str;

    fn render<'a>(&self, poem: &'a Poem) -> Self::Str<'a> {
        &poem.title
    }
}

fn main() -> io::Result<()> {
    let mut picker = PickerOptions::new()
        .highlight_line(true)
        .preview_line_numbers(true)
        .picker(PoemRenderer);
    let injector = picker.injector();

    spawn(move || {
        injector
            .deserialize(&mut Deserializer::from_str(include_str!("poems.json")))
            .unwrap();
    });

    let previewer = SyncPreviewer(|poem: &Poem, buffer: &mut PreviewBuffer| {
        for (index, line) in poem.lines.iter().enumerate() {
            // don't write a trailing newline
            if index != 0 {
                buffer.newline();
            }
            // this is not necessary strictly, but `push_text` here also handles
            // control characters which is convenient for untrusted input
            buffer.push_text(line);
        }
    });

    match picker.with_preview(previewer).pick()? {
        Some(poem) => println!("{}", poem.author),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
