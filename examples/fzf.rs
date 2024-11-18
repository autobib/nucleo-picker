//! # Simple `fzf` clone
//!
//! Read lines from `stdin` in a streaming fashion and populate the picker, imitating the basic
//! functionality of [fzf](https://github.com/junegunn/fzf).
use std::{
    io::{self, BufRead},
    process::exit,
    thread::spawn,
};

use nucleo_picker::{render::StrRenderer, Picker};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    let injector = picker.injector();
    spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(s) => injector.push(s),
                // silently drop io errors!
                Err(_) => {}
            }
        }
    });

    match picker.pick()? {
        Some(it) => println!("{it}"),
        None => exit(1),
    }
    Ok(())
}
