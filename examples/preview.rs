//! # Basic preview example
//!
//! This is a basic synchronous preview example. Run with
//!
//! ```bash
//! cargo run --release --example preview --features serde,preview
//! ```
use std::{convert::Infallible, io, thread::spawn, time::Duration};

use nucleo_picker::{
    PickerOptions, Render,
    preview::{Preview, PreviewRequest, PreviewResponse},
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

struct PoemPreviewer;

impl Preview<Poem> for PoemPreviewer {
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        poem: &Poem,
        request: PreviewRequest,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        let mut buffer = request.ready();
        for (index, line) in poem.lines.iter().enumerate() {
            // don't write a trailing newline
            if index != 0 {
                buffer.newline();
            }
            // this is not necessary strictly, but `push_text` here also handles
            // control characters which is convenient for untrusted input
            buffer.push_text(line);
        }
        Ok(PreviewResponse::Ready(buffer))
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

    match picker.with_preview(PoemPreviewer).pick()? {
        Some(poem) => println!("{}", poem.author),
        None => println!("Nothing selected!"),
    }

    Ok(())
}
