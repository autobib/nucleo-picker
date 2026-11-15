use nucleo_picker::{Picker, Single, UncachedPreview, preview::Pane, render::StrRenderer};

fn main() -> std::io::Result<()> {
    let interface = UncachedPreview::new(Single, |color: &&str, pane: &mut Pane| {
        pane.push_text(color);
    });

    let mut picker = Picker::new(StrRenderer).with_interface(interface);
    picker.push_batch(["red", "blue", "green"]);
    if let Some(color) = picker.pick()? {
        println!("Selected {color}");
    }
    Ok(())
}
