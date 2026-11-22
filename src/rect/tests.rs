use super::*;
use crossterm::style::Stylize;

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
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::WithinRect);
    rect.move_to(2, 0).unwrap();
    rect.move_to_column(4).unwrap();
    rect.next_line().unwrap();
    rect.next_line().unwrap();
    assert_eq!(output, b"\x1b[4;8H\x1b[10G\x1b[5;6H");
}

#[test]
fn empty_rectangles_preserve_dimensions_and_clear_without_output() {
    for (width, height) in [(0, 0), (0, 3), (6, 0)] {
        for clear in [
            ClearState::Precleared,
            ClearState::ToEndOfLine,
            ClearState::WithinRect,
        ] {
            let mut output = Vec::new();
            let mut target = CrosstermTarget::new(&mut output);
            let mut rect = target.rect(
                Area {
                    column: u16::MAX - width,
                    row: u16::MAX - height,
                    width,
                    height,
                },
                clear,
            );
            assert_eq!(rect.width(), width);
            assert_eq!(rect.height(), height);
            assert!(rect.is_empty());
            rect.clear().unwrap();
            rect.clear().unwrap();
            assert!(output.is_empty());
        }
    }
}

#[test]
fn coordinates_allow_the_largest_one_based_ansi_position() {
    for area in [
        Area {
            column: 0,
            row: 0,
            width: u16::MAX,
            height: u16::MAX,
        },
        Area {
            column: u16::MAX - 1,
            row: u16::MAX - 1,
            width: 1,
            height: 1,
        },
    ] {
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        let mut rect = target.rect(area, ClearState::Precleared);
        rect.move_to(area.width - 1, area.height - 1).unwrap();
        rect.move_to_column(area.width - 1).unwrap();
        rect.next_line().unwrap();
        assert_eq!(output, b"\x1b[65535;65535H\x1b[65535G");
    }
}

#[test]
fn construction_checks_each_axis_for_ansi_coordinate_overflow() {
    for area in [
        Area {
            column: u16::MAX,
            row: 0,
            width: 1,
            height: 1,
        },
        Area {
            column: 0,
            row: u16::MAX,
            width: 1,
            height: 1,
        },
        Area {
            column: u16::MAX - 1,
            row: 0,
            width: 2,
            height: 0,
        },
        Area {
            column: 0,
            row: u16::MAX - 1,
            width: 0,
            height: 2,
        },
    ] {
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                target.rect(area, ClearState::WithinRect);
            }))
            .is_err(),
            "{area:?}"
        );
        assert!(output.is_empty());
    }
}

#[test]
fn clearing_respects_origin_policy_and_full_width_lines() {
    for (clear, line_clear) in [
        (ClearState::Precleared, "\x1b[6G"),
        (ClearState::ToEndOfLine, "\x1b[6G\x1b[K"),
        (ClearState::WithinRect, "\x1b[6G        \x1b[6G"),
    ] {
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        target.reset_style().unwrap();
        target.rect(area(), clear).clear().unwrap();
        let expected = if clear == ClearState::Precleared {
            "\x1b[4;6H".to_owned()
        } else {
            format!("\x1b[4;6H{line_clear}\x1b[5;6H{line_clear}\x1b[4;6H")
        };
        assert_eq!(output, format!("\x1b[0m{expected}").as_bytes());
        for text in ["界", "12345678"] {
            output.clear();
            let mut target = CrosstermTarget::new(&mut output);
            target.reset_style().unwrap();
            let mut rect = target.rect(area(), clear);
            rect.clear_line().unwrap();
            rect.text(text, ContentStyle::new()).unwrap();
            rect.next_line().unwrap();
            rect.clear_line().unwrap();
            rect.text("x", ContentStyle::new()).unwrap();
            let expected = format!("\x1b[0m{line_clear}{text}\x1b[5;6H{line_clear}x");
            assert_eq!(output, expected.as_bytes());
        }
    }
}

#[test]
fn spaces_preserve_the_active_style() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.spaces(2, ContentStyle::new()).unwrap();
    rect.spaces(3, ContentStyle::new().on_dark_grey()).unwrap();
    rect.spaces(0, ContentStyle::new().on_red()).unwrap();
    target.reset_style().unwrap();
    assert_eq!(output, b"\x1b[0m  \x1b[48;5;8m   \x1b[49m");
}

#[test]
fn styles_are_complete_and_shared_across_rectangle_borrows() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let styled = ContentStyle::new()
        .bold()
        .red()
        .on_blue()
        .underline(Color::Green);
    for text in ["a", "b"] {
        target
            .rect(area(), ClearState::Precleared)
            .text(text, styled)
            .unwrap();
    }
    target
        .rect(area(), ClearState::Precleared)
        .text("c", ContentStyle::new())
        .unwrap();
    assert_eq!(
        output,
        b"\x1b[0m\x1b[48;5;12m\x1b[38;5;9m\x1b[58;5;10m\x1b[1mab\x1b[0mc",
    );
}

#[test]
fn underline_color_does_not_leak_into_unstyled_text() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.text("a", ContentStyle::new().underline(Color::Green))
        .unwrap();
    rect.text("b", ContentStyle::new()).unwrap();
    assert_eq!(output, b"\x1b[0m\x1b[58;5;10ma\x1b[59mb");
}

#[test]
fn clearing_resets_the_blank_style_but_precleared_rows_only_move() {
    crossterm::style::force_color_output(true);
    for clear in [
        ClearState::Precleared,
        ClearState::ToEndOfLine,
        ClearState::WithinRect,
    ] {
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        let mut rect = target.rect(area(), clear);
        rect.text("x", ContentStyle::new().bold().on_blue())
            .unwrap();
        rect.next_line().unwrap();
        rect.clear_line().unwrap();
        let suffix = match clear {
            ClearState::Precleared => "\x1b[5;6H\x1b[6G",
            ClearState::ToEndOfLine => "\x1b[5;6H\x1b[6G\x1b[0m\x1b[K",
            ClearState::WithinRect => "\x1b[5;6H\x1b[6G\x1b[0m        \x1b[6G",
        };
        assert!(output.ends_with(suffix.as_bytes()), "{output:?}");
    }
}

#[test]
fn empty_writes_do_not_emit_output() {
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.text("", ContentStyle::new().bold()).unwrap();
    rect.spaces(0, ContentStyle::new().red()).unwrap();
    rect.ascii(format_args!(""), ContentStyle::new().green())
        .unwrap();
    rect.styled_text("", std::iter::empty()).unwrap();
    assert!(output.is_empty());
}

#[test]
fn writer_errors_propagate_without_further_writes() {
    #[derive(Default)]
    struct Fails {
        output: Vec<u8>,
        failed: bool,
    }

    impl io::Write for Fails {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            assert!(!self.failed, "drawing continued after failure");
            let len = bytes
                .iter()
                .position(|&byte| byte == b'b')
                .unwrap_or(bytes.len());
            if len == 0 {
                self.failed = true;
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.output.extend_from_slice(&bytes[..len]);
            Ok(len)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut writer = Fails::default();
    let mut target = CrosstermTarget::new(&mut writer);
    let mut rect = target.rect(area(), ClearState::Precleared);
    let error = rect
        .styled_text(
            "abc",
            ["a", "b", "c"]
                .into_iter()
                .map(|text| ContentStyle::new().apply(text)),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(writer.output.ends_with(b"a"));
}
