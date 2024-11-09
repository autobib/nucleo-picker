//! # Rendering with mutable internal state
//!
//! This example demonstrates how one might use internal mutable state to improve rendering
//! performance. In reality, you probably should not be performing this type of optimization and
//! should only do so if benchmarks demonstrate that it is necessary!
//!
//! Indeed, in this case, simply using the default [`Display`](std::fmt::Display) implementation
//! results in essentially identical runtime in practice since the vast majority of time is spent
//! moving and allocating state within the matcher or performing matching, rather than in the
//! rendering of the types for display.
use std::{collections::HashSet, io::Result, thread::spawn};

use rand::{distributions::Standard, rngs::StdRng, Rng, SeedableRng};
use ryu::{Buffer, Float};

use nucleo_picker::{PickerOptions, Render};

#[derive(Clone, Default)]
pub struct FloatRender {
    buffer: Buffer,
}

impl<F: Float> Render<F> for FloatRender {
    type Column<'a>
        = &'a str
    where
        F: 'a;

    fn as_column<'a>(&'a mut self, value: &'a F) -> Self::Column<'a> {
        // render the value into the buffer, and return output which is borrowed from the internal
        // buffer
        self.buffer.format(*value)
    }
}

fn main() -> Result<()> {
    let mut selected_keys: HashSet<String> = HashSet::new();
    selected_keys.insert("year".to_owned());
    selected_keys.insert("month".to_owned());

    let mut picker = PickerOptions::default().picker(FloatRender::default());
    // In practice, it is sufficient to:
    //  let mut picker = PickerOptions::default().picker(nucleo_picker::render::DisplayRender);

    let mut injector = picker.injector();

    spawn(move || {
        let mut rnd = StdRng::seed_from_u64(0);
        for _ in 0..1000000 {
            let val: f64 = rnd.sample(Standard);
            injector.push(val);
        }
    });

    match picker.pick()? {
        Some(f) => println!("Your number: {f}"),
        None => println!("No number chosen!"),
    }

    Ok(())
}
