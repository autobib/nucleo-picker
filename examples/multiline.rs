//! # Unicode rendering and multiline support
//!
//! This example demonstrates how unicode matching and multi-line matching is handled inside the
//! TUI.
use std::io::Result;

use nucleo_picker::{render::StrRenderer, Picker};

fn main() -> Result<()> {
    let mut picker = Picker::new(StrRenderer);

    let choices = vec![
        "0",
        "01",
        "012",
        "0123",
        "01234",
        "012345",
        "0123456",
        "01234567",
        "012345678",
        "0123456789",
        "01234567890",
        "012345678901",
        "0123456789012",
        "01234567890123",
        "012345678901234",
        "0123456789012345",
        "01234567890123456",
        "012345678901234567",
        "0123456789012345678",
        "01234567890123456789",
        "012345678901234567890",
        "0123456789012345678901",
        "01234567890123456789012",
        "Ｈｅｌｌｏ, ｗｏｒｌｄ!",
        "match\n  with\r\n  newline\n",
        "extremely lｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏng\n  ｏｏｏｏｏｏｏｏｏｏｏｏ",
        "xtremely lｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏｏnng\n  ｏｏｏｏｏｏｏｏｏｏｏｏ",
    ];

    let mut repeat_choices: Vec<&'static str> = Vec::new();
    for _ in 0..1 {
        repeat_choices.extend(choices.iter());
    }

    let injector = picker.injector();
    for opt in repeat_choices {
        injector.push(opt);
    }

    // open interactive prompt
    match picker.pick()? {
        Some(opt) => {
            println!("You selected: '{opt}'");
        }
        None => {
            println!("Nothing selected!");
        }
    }

    Ok(())
}
