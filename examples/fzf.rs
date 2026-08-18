//! # A more complete `fzf` clone
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
//!
//! This is a more complete version of the basic fzf example.
use std::{
    fmt,
    io::{self, BufRead, IsTerminal},
    num::NonZero,
    process::exit,
    thread::spawn,
};
#[cfg(feature = "tracing")]
use std::{fs::File, path::Path, sync::Mutex};

use clap::{Parser, ValueEnum};
use nucleo_picker::{CaseMatching, Normalization, PickerOptions, render::StrRenderer};
#[cfg(feature = "tracing")]
use tracing_subscriber::{Layer, fmt::format::FmtSpan, layer::SubscriberExt};

#[derive(Debug, Clone, Default, ValueEnum)]
enum Layout {
    /// Display from the bottom of the screen.
    #[default]
    Default,
    /// Display from the top of the screen.
    Reverse,
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layout::Default => "default",
            Layout::Reverse => "reverse",
        })
    }
}

/// A basic fzf clone with support for a few options.
#[derive(Parser)]
struct Args {
    /// Reverse the order of the input items.
    #[arg(long)]
    tac: bool,

    /// Set the picker interface layout.
    #[arg(long, default_value_t, conflicts_with = "reverse")]
    layout: Layout,

    /// Use reverse layout (same as --layout=reverse).
    #[arg(long, conflicts_with = "layout")]
    reverse: bool,

    /// Disable sorting of results.
    #[arg(long)]
    no_sort: bool,

    /// Enable case-insensitive matching.
    #[arg(short = 'i', long, group = "case_matching")]
    ignore_case: bool,

    /// Force case-sensitive matching.
    #[arg(long, group = "case_matching")]
    no_ignore_case: bool,

    /// Enable smart-case matching, which is case-insensitive by default but bcomes case-sensitive
    /// if the query contains any uppercase letters.
    #[arg(long, group = "case_matching")]
    smart_case: bool,

    /// Do not normalie latin script letters.
    #[arg(long)]
    literal: bool,

    /// Set an initial query string.
    #[arg(short = 'q', long, default_value = "")]
    query: String,

    /// Enable multi-select mode with optional max selection count.
    #[arg(short = 'm', long, value_name = "MAX", num_args = 0..=1, conflicts_with = "no_multi")]
    multi: Option<Option<NonZero<u32>>>,

    /// Disable multi-select mode.
    #[arg(long, conflicts_with = "multi")]
    no_multi: bool,

    /// Split input using null characters instead of newlines.
    #[arg(long)]
    read0: bool,

    /// Write picker frame tracing spans as JSON Lines.
    #[cfg(feature = "tracing")]
    #[arg(long, value_name = "PATH", hide = true)]
    tracing_output: Option<std::path::PathBuf>,
}

#[cfg(feature = "tracing")]
fn tracing_subscriber(path: &Path) -> io::Result<impl tracing::Subscriber + Send + Sync> {
    let writer = Mutex::new(File::create(path)?);
    let frames = tracing_subscriber::filter::filter_fn(|metadata| {
        metadata.is_span()
            && metadata.target() == "nucleo_picker::frame"
            && metadata.name() == "picker.frame"
            && *metadata.level() == tracing::Level::DEBUG
    });
    Ok(tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(writer)
            .with_filter(frames),
    ))
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    #[cfg(feature = "tracing")]
    if let Some(path) = &args.tracing_output {
        tracing::subscriber::set_global_default(tracing_subscriber(path)?)
            .map_err(io::Error::other)?;
    }

    let options = PickerOptions::new()
        .reverse_items(args.tac)
        .sort_results(!args.no_sort)
        .max_selection_count(args.multi.flatten())
        .normalization(if args.literal {
            Normalization::Never
        } else {
            Normalization::Smart
        })
        .case_matching(if args.ignore_case {
            CaseMatching::Ignore
        } else if args.no_ignore_case {
            CaseMatching::Respect
        } else {
            CaseMatching::Smart
        })
        .reversed(args.reverse || matches!(args.layout, Layout::Reverse))
        .query(args.query);

    let mut picker = options.picker(StrRenderer);

    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin().lock();
        if !stdin.is_terminal() {
            if args.read0 {
                for chunk in stdin.split(b'\0') {
                    // silently drop IO and utf8 conversion errors!
                    if let Ok(bytes) = chunk
                        && !bytes.is_empty()
                        && let Ok(s) = String::from_utf8(bytes)
                    {
                        injector.push(s)
                    }
                }
            } else {
                for line in stdin.lines() {
                    // silently drop IO errors!
                    if let Ok(s) = line {
                        injector.push(s);
                    }
                }
            }
        }
    });

    if args.no_multi {
        match picker.pick()? {
            Some(it) => println!("{it}"),
            None => exit(1),
        }
    } else {
        let selection = picker.pick_multi()?;
        if selection.is_empty() {
            exit(1);
        }
        for item in selection.iter() {
            println!("{item}");
        }
    }
    Ok(())
}
