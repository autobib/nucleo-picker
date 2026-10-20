use std::{fmt::Write, io, time::Duration};

use crossterm::style::{ContentStyle, Stylize};
use nucleo_picker::{
    Picker, Render,
    preview::{
        Preview, WithPreview,
        request::{PreviewRequest, PreviewResponse},
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
    fn preview(
        &mut self,
        request: PreviewRequest<'_, Color>,
        _timeout: Duration,
    ) -> PreviewResponse {
        self.generated += 1;
        let style = match request.item() {
            Color::Red => ContentStyle::new().on_red(),
            Color::Blue => ContentStyle::new().on_blue(),
            Color::Green => ContentStyle::new().on_green(),
            Color::Yellow => ContentStyle::new().on_yellow(),
            Color::Magenta => ContentStyle::new().on_magenta(),
        };
        let mut buffer = request.ready();
        let _ = writeln!(buffer.text_writer(), "Preview #{}", self.generated);
        buffer.push_styled_text("        \n        \n        ", style);
        PreviewResponse::Ready(buffer)
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
        .as_interface(WithPreview::new_single(&mut previewer))
        .pick()?;

    // the lifetime of 'selected' is tied only to the picker itself, so we can now
    // access the previewer (even mutably if needed) without dropping 'selected'
    println!("Generated {} previews while running", previewer.generated);

    if let Some(color) = selected {
        println!("{color:?}");
    }
    Ok(())
}
