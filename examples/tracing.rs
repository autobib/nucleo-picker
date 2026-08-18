//! # Tracing example
//!
//! This is identical to the `multi` example but records picker frame spans to a specified file.
use std::{env, fs::File, io, num::NonZero, sync::Mutex};

use nucleo_picker::{PickerOptions, render::StrRenderer};
use tracing_subscriber::{Layer, fmt::format::FmtSpan, prelude::*};

fn main() -> io::Result<()> {
    let path = env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing trace output path; usage: tracing PATH",
        )
    })?;
    let trace = File::create(path)?;
    let frames = tracing_subscriber::filter::filter_fn(|metadata| {
        metadata.is_span()
            && metadata.target() == "nucleo_picker::frame"
            && metadata.name() == "picker.frame"
            && *metadata.level() == tracing::Level::DEBUG
    });
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::CLOSE)
                .with_target(true)
                .with_writer(Mutex::new(trace))
                .with_filter(frames),
        )
        .init();

    let mut picker = PickerOptions::new()
        // allow at most 3 selections
        .max_selection_count(NonZero::new(3))
        .picker(StrRenderer);

    picker.push_batch([
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
