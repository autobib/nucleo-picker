use crossterm::style::{Color, SetForegroundColor};

use crate::{
    rect::{Area, ClearState, CrosstermTarget},
    render::StrRenderer,
    session::{
        DriverOptions,
        tests::{Select, frame_options},
    },
};

fn render(text: &str) -> String {
    let mut driver = DriverOptions::new()
        .query("ab")
        .list_reversed(true)
        .driver(StrRenderer);
    driver.push_batch([text.to_owned()]);
    assert_eq!(driver.filter().unwrap().len(), 1);
    let mut session = driver.session(Select);
    let mut frame = session.prepare_frame(frame_options());
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(
        Area {
            width: 20,
            height: 4,
            ..Area::default()
        },
        ClearState::Precleared,
    );
    frame.match_list().draw(&mut rect).unwrap();
    frame.complete();
    String::from_utf8(output).unwrap()
}

#[test]
fn crlf_and_lf_items_render_the_same_highlights_and_line_boundaries() {
    crossterm::style::force_color_output(true);
    for text in [
        "a\r\nb",
        "a\r\n\r\nb",
        "a\n\r\nb",
        "a\r\nb\r\n",
        "a\r\n界b",
        "a\r\nu\u{0308}b",
    ] {
        let output = render(text);
        assert_eq!(output, render(&text.replace("\r\n", "\n")));
        assert!(output.contains(&format!("{}a", SetForegroundColor(Color::Cyan))));
        assert!(output.contains(&format!("{}b", SetForegroundColor(Color::Cyan))));
        assert!(!output.contains(['\r', '\n']));
    }
}
