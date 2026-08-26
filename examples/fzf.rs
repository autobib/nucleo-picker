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

use clap::{Parser, ValueEnum};
use nucleo_picker::{CaseMatching, Normalization, PickerOptions, render::StrRenderer};

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
            Self::Default => "default",
            Self::Reverse => "reverse",
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
}

fn main() -> io::Result<()> {
    let args = Args::parse();

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
                        injector.push(s);
                    }
                }
            } else {
                for s in stdin.lines().map_while(Result::ok) {
                    // silently drop IO errors!
                    injector.push(s);
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
