//! # `find`-style picker with aggregate frame timing
//!
//! This is identical to the `find` example, but uses `tracing-timing` to print aggregate picker
//! timing statistics after the picker exits.
use std::{
    borrow::Cow, env::args, io, path::PathBuf, process::exit, thread::spawn, time::Duration,
};

use ignore::{DirEntry, WalkBuilder, WalkState};
use nucleo_picker::{PickerOptions, Render};
use tracing::{Dispatch, dispatcher};
use tracing_subscriber::{Layer, filter::filter_fn, layer::SubscriberExt};
use tracing_timing::{
    Builder, Histogram, LayerDowncaster,
    group::{ByMessage, ByName},
};

pub struct DirEntryRender;

impl Render<DirEntry> for DirEntryRender {
    type Str<'a> = Cow<'a, str>;

    fn render<'a>(&self, value: &'a DirEntry) -> Self::Str<'a> {
        value.path().to_string_lossy()
    }
}

fn print_timing_stats(downcaster: &LayerDowncaster<ByName, ByMessage>, dispatch: &Dispatch) {
    let timing = downcaster.downcast(dispatch).unwrap();
    timing.force_synchronize();
    timing.with_histograms(|histograms| {
        let duration = |nanoseconds| Duration::from_nanos(nanoseconds);
        for (name, label) in [
            ("picker.frame", "whole frame"),
            ("picker.frame.render", "frame rendering"),
            ("picker.match.tick", "matcher update"),
            ("picker.event.wait", "event wait"),
            ("picker.event.handle", "event processing"),
        ] {
            let Some(histogram) = histograms.get(name).and_then(|events| events.get("close"))
            else {
                eprintln!("{label}: no samples recorded");
                continue;
            };
            eprintln!("{label} ({} samples):", histogram.len());
            eprintln!("  mean: {:?}", duration(histogram.mean() as u64));
            eprintln!("  p50:  {:?}", duration(histogram.value_at_quantile(0.50)));
            eprintln!("  p90:  {:?}", duration(histogram.value_at_quantile(0.90)));
            eprintln!("  p99:  {:?}", duration(histogram.value_at_quantile(0.99)));
            eprintln!("  max:  {:?}", duration(histogram.max()));
        }
    });
}

fn main() -> io::Result<()> {
    let timing = Builder::default()
        .no_span_recursion()
        .span_close_events()
        .layer(|| Histogram::new_with_max(60_000_000_000, 3).unwrap());
    let downcaster = timing.downcaster();
    let timing_filter = filter_fn(|metadata| {
        metadata.is_span()
            && matches!(
                metadata.name(),
                "picker.frame"
                    | "picker.frame.render"
                    | "picker.match.tick"
                    | "picker.event.wait"
                    | "picker.event.handle"
            )
    });
    let subscriber = tracing_subscriber::registry().with(timing.with_filter(timing_filter));
    let dispatch = Dispatch::new(subscriber);
    dispatcher::set_global_default(dispatch.clone()).map_err(io::Error::other)?;

    let mut picker = PickerOptions::default()
        .match_paths()
        .picker(DirEntryRender);

    let root: PathBuf = args().nth(1).map_or_else(|| ".".into(), Into::into);
    let injector = picker.injector();
    spawn(move || {
        WalkBuilder::new(root).build_parallel().run(|| {
            let injector = injector.clone();
            Box::new(move |walk_res| {
                if let Ok(dir) = walk_res {
                    injector.push(dir);
                }
                WalkState::Continue
            })
        });
    });

    let selected = picker.pick()?;
    print_timing_stats(&downcaster, &dispatch);

    match selected {
        Some(entry) => println!("{}", entry.path().display()),
        None => {
            eprintln!("No path selected!");
            exit(1);
        }
    }

    Ok(())
}
