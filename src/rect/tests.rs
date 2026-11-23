use super::*;
use crate::draw::Attributes;

fn area() -> Area {
    Area {
        column: 5,
        row: 3,
        width: 8,
        height: 2,
    }
}

#[test]
fn empty_rectangles_preserve_dimensions_without_output() {
    for (width, height) in [(0, 0), (0, 3), (6, 0)] {
        for clear in [
            ClearState::Precleared,
            ClearState::ToEndOfLine,
            ClearState::WithinRect,
        ] {
            let mut output = Vec::new();
            let mut target = CrosstermTarget::new(&mut output);
            let rect = target.rect(
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
        rect.begin_row(area.height - 1, true).unwrap();
        assert_eq!(
            output,
            format!("\x1b[65535;{}H", area.column + 1).as_bytes()
        );
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
        (ClearState::Precleared, ""),
        (ClearState::ToEndOfLine, "\x1b[K"),
        (ClearState::WithinRect, "        \x1b[6G"),
    ] {
        let mut output = Vec::new();
        let mut target = CrosstermTarget::new(&mut output);
        target.reset_style().unwrap();
        let mut rect = target.rect(area(), clear);
        rect.begin_row(0, true).unwrap();
        rect.begin_row(1, true).unwrap();
        let expected = format!("\x1b[4;6H{line_clear}\x1b[5;6H{line_clear}");
        assert_eq!(output, format!("\x1b[0m{expected}").as_bytes());
        for text in ["界", "12345678"] {
            output.clear();
            let mut target = CrosstermTarget::new(&mut output);
            target.reset_style().unwrap();
            let mut rect = target.rect(area(), clear);
            rect.begin_row(0, true).unwrap();
            rect.text(text, Style::new()).unwrap();
            rect.begin_row(1, true).unwrap();
            rect.text("x", Style::new()).unwrap();
            let expected = format!("\x1b[0m\x1b[4;6H{line_clear}{text}\x1b[5;6H{line_clear}x");
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
    rect.begin_row(0, true).unwrap();
    rect.spaces(2, Style::new()).unwrap();
    rect.spaces(3, Style::new().bg(Color::DarkGrey)).unwrap();
    rect.spaces(0, Style::new().bg(Color::Red)).unwrap();
    target.reset_style().unwrap();
    assert_eq!(output, b"\x1b[4;6H\x1b[0m  \x1b[48;5;8m   \x1b[49m");
}

#[test]
fn styles_are_complete_and_shared_across_rectangle_borrows() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let styled = Style::new()
        .bold()
        .fg(Color::Red)
        .bg(Color::Blue)
        .underline_color(Color::Green);
    for (row, text) in [(3, "a"), (4, "b")] {
        let mut rect = target.rect(Area { row, ..area() }, ClearState::Precleared);
        rect.begin_row(0, true).unwrap();
        rect.text(text, styled).unwrap();
    }
    let mut rect = target.rect(Area { row: 5, ..area() }, ClearState::Precleared);
    rect.begin_row(0, true).unwrap();
    rect.text("c", Style::new()).unwrap();
    assert_eq!(
        output,
        b"\x1b[4;6H\x1b[0m\x1b[48;5;12m\x1b[38;5;9m\x1b[58;5;10m\x1b[1ma\x1b[5;6Hb\x1b[6;6H\x1b[0mc",
    );
}

#[test]
fn adding_single_underline_preserves_existing_double_underline() {
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.begin_row(0, true).unwrap();
    let style = Style::new().attributes(Attributes::DOUBLE_UNDERLINED);
    rect.text("a", style).unwrap();
    let combined = style.underlined();
    rect.text("b", combined).unwrap();
    rect.text("c", combined).unwrap();
    assert_eq!(output, b"\x1b[4;6H\x1b[0m\x1b[4:2ma\x1b[4m\x1b[4:2mbc");
}

#[test]
fn removing_attributes_restores_all_colors_and_remaining_attributes() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.begin_row(0, true).unwrap();
    let style = Style::new()
        .bold()
        .italic()
        .fg(Color::Red)
        .bg(Color::Blue)
        .underline_color(Color::Green);
    rect.text("a", style).unwrap();
    rect.text("b", style.remove_attributes(Attributes::BOLD))
        .unwrap();
    assert_eq!(
        output,
        concat!(
            "\x1b[4;6H",
            "\x1b[0m\x1b[48;5;12m\x1b[38;5;9m\x1b[58;5;10m\x1b[1m\x1b[3ma",
            "\x1b[0m\x1b[48;5;12m\x1b[38;5;9m\x1b[58;5;10m\x1b[3mb",
        )
        .as_bytes(),
    );
}

#[test]
fn imported_off_commands_and_resets_render_as_absolute_styles() {
    use crossterm::style::{Attribute, Color as CrosstermColor, ContentStyle};

    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.begin_row(0, true).unwrap();
    let mut style = ContentStyle {
        foreground_color: Some(CrosstermColor::Red),
        background_color: Some(CrosstermColor::Blue),
        underline_color: Some(CrosstermColor::Green),
        attributes: Attribute::NoItalic.into(),
    };
    rect.text("a", style.into()).unwrap();
    style.attributes.set(Attribute::Italic);
    rect.text("b", style.into()).unwrap();
    style.attributes.set(Attribute::Reset);
    rect.text("c", style.into()).unwrap();
    style.foreground_color = Some(CrosstermColor::Yellow);
    style.background_color = Some(CrosstermColor::Cyan);
    style.underline_color = Some(CrosstermColor::Magenta);
    rect.text("d", style.into()).unwrap();
    assert_eq!(
        output,
        b"\x1b[4;6H\x1b[0m\x1b[48;5;12m\x1b[38;5;9m\x1b[58;5;10mab\x1b[49m\x1b[39m\x1b[59mcd",
    );
}

#[test]
fn underline_color_does_not_leak_into_unstyled_text() {
    crossterm::style::force_color_output(true);
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.begin_row(0, true).unwrap();
    rect.text("a", Style::new().underline_color(Color::Green))
        .unwrap();
    rect.text("b", Style::new()).unwrap();
    assert_eq!(output, b"\x1b[4;6H\x1b[0m\x1b[58;5;10ma\x1b[59mb");
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
        rect.begin_row(0, false).unwrap();
        rect.spaces(8, Style::new().bold().bg(Color::Blue)).unwrap();
        rect.begin_row(1, true).unwrap();
        let suffix = match clear {
            ClearState::Precleared => "\x1b[5;6H",
            ClearState::ToEndOfLine => "\x1b[5;6H\x1b[0m\x1b[K",
            ClearState::WithinRect => "\x1b[5;6H\x1b[0m        \x1b[6G",
        };
        assert!(output.ends_with(suffix.as_bytes()), "{output:?}");
    }
}

#[test]
fn empty_writes_do_not_emit_output() {
    let mut output = Vec::new();
    let mut target = CrosstermTarget::new(&mut output);
    let mut rect = target.rect(area(), ClearState::Precleared);
    rect.text("", Style::new().bold()).unwrap();
    rect.spaces(0, Style::new().fg(Color::Red)).unwrap();
    rect.ascii(format_args!(""), Style::new().fg(Color::Green))
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
    rect.begin_row(0, true).unwrap();
    let error = rect
        .styled_text(
            "abc",
            ["a", "b", "c"]
                .into_iter()
                .map(|text| Style::new().apply(text)),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(writer.output.ends_with(b"a"));
}
