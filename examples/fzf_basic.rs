//! # Simple `fzf` clone
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
use std::{
    io::{self, IsTerminal},
    process::exit,
    thread::spawn,
};

use nucleo_picker::{Picker, render::StrRenderer};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            for s in stdin.lines().map_while(Result::ok) {
                // silently drop IO errors!
                injector.push(s);
            }
        }
    });

    match picker.pick()? {
        Some(it) => println!("{it}"),
        None => exit(1),
    }
    Ok(())
}
