use crossterm::style::Stylize;
use std::time::Duration;

use crate::{
    DriverOptions,
    rect::Area,
    render::StrRenderer,
    session::{
        Dimensions, Geometry, Redraw,
        tests::{Select, TestTerminal, frame_options},
    },
};

fn render(text: &str) -> String {
    let mut driver = DriverOptions::new()
        .query("ab")
        .reversed(true)
        .driver(StrRenderer);
    driver.push_batch([text.to_owned()]);
    assert_eq!(driver.filter().unwrap().len(), 1);
    let mut session = driver.session(Select, Duration::ZERO);
    let mut frame = session.prepare_frame(frame_options());
    frame.resize(Geometry {
        dimensions: Dimensions {
            width: 20,
            height: 4,
        },
        list: Area {
            width: 20,
            height: 4,
            ..Area::default()
        },
        ..Geometry::default()
    });
    let mut terminal = TestTerminal::default();
    frame.draw(&mut terminal, Redraw::FULL).unwrap();
    frame.complete();
    String::from_utf8(terminal.output).unwrap()
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
        assert!(output.contains(&"a".cyan().to_string()));
        assert!(output.contains(&"b".cyan().to_string()));
        assert!(!output.contains(['\r', '\n']));
    }
}
