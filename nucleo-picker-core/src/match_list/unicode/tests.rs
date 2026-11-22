use crate::{
    component::tests::TestSurface,
    draw::Color,
    render::StrRenderer,
    session::{
        ModelOptions,
        tests::{Select, frame_options},
    },
};

fn render(text: &str) -> TestSurface {
    let mut model = ModelOptions::new()
        .query("ab")
        .list_reversed(true)
        .model(StrRenderer);
    model.push_batch([text.to_owned()]);
    assert_eq!(model.filter().unwrap().len(), 1);
    let mut session = model.session(Select);
    let mut frame = session.prepare_frame(frame_options());
    let mut rect = TestSurface::new(20, 4);
    frame.match_list().draw(&mut rect).unwrap();
    frame.complete();
    rect
}

#[test]
fn crlf_and_lf_items_render_the_same_highlights_and_line_boundaries() {
    for text in [
        "a\r\nb",
        "a\r\n\r\nb",
        "a\n\r\nb",
        "a\r\nb\r\n",
        "a\r\n界b",
        "a\r\nu\u{0308}b",
    ] {
        let output = render(text);
        let normalized = render(&text.replace("\r\n", "\n"));
        assert_eq!(output.rows, normalized.rows);
        assert_eq!(output.spans, normalized.spans);
        for ch in ['a', 'b'] {
            assert!(output.spans.iter().any(|(text, style)| {
                text.contains(ch) && style.foreground_color == Some(Color::Cyan)
            }));
        }
        assert!(output.rows.iter().all(|row| !row.contains(['\r', '\n'])));
    }
}
