use super::*;

fn area() -> Area {
    Area {
        column: 5,
        row: 3,
        width: 8,
        height: 2,
    }
}

#[test]
fn movement_uses_local_coordinates_and_stops_at_the_last_row() {
    let mut output = Vec::new();
    let mut rect = CrosstermRect::new(&mut output, area(), ClearState::WithinRect).unwrap();
    rect.move_to(2, 0).unwrap();
    rect.move_to_column(4).unwrap();
    rect.next_line().unwrap();
    rect.next_line().unwrap();
    assert_eq!(output, b"\x1b[4;8H\x1b[10G\x1b[5;6H");
}

#[test]
fn clearing_respects_origin_policy_and_full_width_lines() {
    for (clear, line_clear) in [
        (ClearState::Precleared, "\x1b[6G"),
        (ClearState::ToEndOfLine, "\x1b[6G\x1b[K"),
        (ClearState::WithinRect, "\x1b[6G        \x1b[6G"),
    ] {
        let mut output = Vec::new();
        CrosstermRect::new(&mut output, area(), clear)
            .unwrap()
            .clear()
            .unwrap();
        let expected = if clear == ClearState::Precleared {
            "\x1b[4;6H".to_owned()
        } else {
            format!("\x1b[4;6H{line_clear}\x1b[5;6H{line_clear}\x1b[4;6H")
        };
        assert_eq!(output, expected.as_bytes());
        for text in ["界", "12345678"] {
            output.clear();
            let mut rect = CrosstermRect::new(&mut output, area(), clear).unwrap();
            rect.clear_line().unwrap();
            rect.print(text).unwrap();
            rect.next_line().unwrap();
            rect.clear_line().unwrap();
            rect.print("x").unwrap();
            let expected = format!("{line_clear}{text}\x1b[5;6H{line_clear}x");
            assert_eq!(output, expected.as_bytes());
        }
    }
}

#[test]
fn spaces_preserve_the_active_style() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut rect = CrosstermRect::new(&mut output, area(), ClearState::Precleared).unwrap();
    rect.spaces(2).unwrap();
    rect.set_background(Color::DarkGrey).unwrap();
    rect.spaces(3).unwrap();
    rect.reset_style().unwrap();
    rect.spaces(0).unwrap();
    assert_eq!(output, b"  \x1b[48;5;8m   \x1b[0m\x1b[0m");
}

#[test]
fn drawing_propagates_writer_errors() {
    struct FailingWriter;
    impl io::Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("rectangle drawing must not flush");
        }
    }
    let mut writer = FailingWriter;
    let mut rect = CrosstermRect::new(&mut writer, area(), ClearState::WithinRect).unwrap();
    assert_eq!(
        rect.move_to(0, 0).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        rect.print("text").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        rect.clear_line().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(rect.clear().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}
