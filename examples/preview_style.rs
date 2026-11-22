use std::{fmt::Write, io, time::Duration};

use nucleo_picker::{
    Picker, Render,
    draw::{Color as PickerColor, Style},
    preview::{
        CachedPreview, Preview,
        request::{Request, Response},
    },
};

#[derive(Debug)]
enum Color {
    Red,
    Blue,
    Green,
    Yellow,
    Magenta,
}

// render the color as its own text
struct ColorRenderer;

impl Render<Color> for ColorRenderer {
    type Str<'a> = &'static str;

    fn render<'a>(&self, item: &'a Color) -> Self::Str<'a> {
        match item {
            Color::Red => "red",
            Color::Blue => "blue",
            Color::Green => "green",
            Color::Yellow => "yellow",
            Color::Magenta => "magenta",
        }
    }
}

// a previewer which keeps track of how many times
// it was called
#[derive(Default)]
struct ColorPreview {
    generated: usize,
}

impl Preview<Color> for ColorPreview {
    // generate the preview with a color swatch
    // and with a header indicating the index at which
    // it was generated
    fn preview(&mut self, request: Request<'_, Color>, _timeout: Duration) -> Response {
        self.generated += 1;
        let style = match request.item() {
            Color::Red => Style::new().bg(PickerColor::Red),
            Color::Blue => Style::new().bg(PickerColor::Blue),
            Color::Green => Style::new().bg(PickerColor::Green),
            Color::Yellow => Style::new().bg(PickerColor::Yellow),
            Color::Magenta => Style::new().bg(PickerColor::Magenta),
        };
        let mut pane = request.ready();
        let _ = writeln!(pane.text_writer(), "Preview #{}", self.generated);
        pane.push_styled_text("        \n        \n        ", style);
        Response::Ready(pane)
    }
}

fn main() -> io::Result<()> {
    let mut picker = Picker::new(ColorRenderer);
    picker.push_batch([
        Color::Red,
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
    ]);
    let mut previewer = ColorPreview::default();

    // get the selection by running the picker with the previewer
    let selected = picker
        .as_interface(CachedPreview::new_single(&mut previewer))
        .pick()?;

    // the lifetime of 'selected' is tied only to the picker itself, so we can now
    // access the previewer (even mutably if needed) without dropping 'selected'
    println!("Generated {} previews while running", previewer.generated);

    if let Some(color) = selected {
        println!("{color:?}");
    }
    Ok(())
}
