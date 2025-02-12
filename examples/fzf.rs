//! # Simple `fzf` clone
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
use std::{
    io::{self, IsTerminal},
    process::exit,
    thread::spawn,
};

use nucleo_picker::{render::StrRenderer, Picker};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

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
