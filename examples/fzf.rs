//! # Simple `fzf` clone
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
use std::{
    io::{self, IsTerminal},
    process::exit,
    thread::spawn,
};

use argh::FromArgs;
use nucleo_picker::{CaseMatching, PickerOptions, render::StrRenderer};

/// A basic fzf clone with support for a few options.
#[derive(FromArgs)]
struct Args {
    /// reverse the order of input items
    #[argh(switch)]
    tac: bool,

    /// layout: 'default' or 'reverse'
    #[argh(option, default = "String::from(\"default\")")]
    layout: String,

    /// disable sorting of results
    #[argh(switch)]
    no_sort: bool,

    /// case-insensitive matching
    #[argh(switch, short = 'i')]
    ignore_case: bool,

    /// initial query string
    #[argh(option, short = 'q', default = "String::new()")]
    query: String,
}

fn main() -> io::Result<()> {
    let args: Args = argh::from_env();

    // Configure picker options based on command-line flags
    let mut options = PickerOptions::new()
        .reverse_items(args.tac)
        .sort_results(!args.no_sort)
        .query(args.query);

    if args.layout == "reverse" {
        options = options.reversed(true);
    } else if args.layout != "default" {
        eprintln!(
            "Invalid layout option: {}. Valid choices are 'default' or 'reverse'",
            args.layout
        );
        exit(1);
    }

    if args.ignore_case {
        options = options.case_matching(CaseMatching::Ignore);
    }

    let mut picker = options.picker(StrRenderer);

    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            for line in stdin.lines() {
                // silently drop IO errors!
                if let Ok(s) = line {
                    injector.push(s);
                }
            }
        }
    });

    match picker.pick()? {
        Some(it) => println!("{it}"),
        None => exit(1),
    }
    Ok(())
}
