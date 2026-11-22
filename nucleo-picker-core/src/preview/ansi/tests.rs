use std::io::Write;

use crate::draw::{Attributes, Style, StyledStr};

use crate::preview::{AnsiState, Buffer};

use super::Color;

fn assert_spans(input: std::fmt::Arguments<'_>, expected: &[StyledStr<'_>]) {
    let mut buffer = Buffer::new();
    buffer.ansi_writer().write_fmt(input).unwrap();
    assert_eq!(buffer.lines().len(), 1);
    assert!(
        buffer.line(0).unwrap().spans().eq(expected.iter().cloned()),
        "{input}"
    );
}

#[test]
fn owned_and_caller_owned_streams_preserve_every_byte_boundary() {
    let input = "\x1b[1;31mé界e\u{301}👩‍💻\x1b[0m!\r\n\t\x1b[38;2;1;2;3mend".as_bytes();
    for chunk_size in 1..=input.len() {
        let mut owned = Buffer::new();
        let mut caller_owned = Buffer::new();
        for buffer in [&mut owned, &mut caller_owned] {
            buffer.push_str("prefix ");
        }
        let mut state = AnsiState::new();
        {
            let mut writer = owned.ansi_writer();
            for chunk in input.chunks(chunk_size) {
                assert_eq!(writer.write(chunk).unwrap(), chunk.len());
                writer.flush().unwrap();
                caller_owned.push_ansi(chunk, &mut state);
            }
        }
        caller_owned.push_ansi(b"", &mut state);
        for buffer in [&owned, &caller_owned] {
            assert_eq!(buffer.lines().len(), 2);
            assert!(
                buffer.line(0).unwrap().spans().eq([
                    Style::default().apply("prefix "),
                    Style::new()
                        .fg(Color::AnsiValue(1))
                        .bold()
                        .apply("é界e\u{301}👩‍💻"),
                    Style::default().apply("!"),
                ]),
                "chunk size {chunk_size}"
            );
            assert!(
                buffer.line(1).unwrap().spans().eq([
                    Style::default().apply("␉"),
                    Style::new()
                        .fg(Color::Rgb { r: 1, g: 2, b: 3 })
                        .apply("end"),
                ]),
                "chunk size {chunk_size}"
            );
        }
    }
}

#[test]
fn attributes_replace_modes_and_reset_selectively_or_fully() {
    let cases = [
        ("1", 22, Attributes::BOLD),
        ("2", 22, Attributes::DIM),
        ("1;2", 22, Attributes::BOLD | Attributes::DIM),
        ("3", 23, Attributes::ITALIC),
        ("4", 24, Attributes::UNDERLINED),
        ("21", 24, Attributes::DOUBLE_UNDERLINED),
        ("5", 25, Attributes::SLOW_BLINK),
        ("6", 25, Attributes::RAPID_BLINK),
        ("7", 27, Attributes::REVERSE),
        ("8", 28, Attributes::HIDDEN),
        ("9", 29, Attributes::CROSSED_OUT),
    ];
    let base = Style::new().bg(Color::AnsiValue(4));
    for (set, reset, attributes) in cases {
        let styled = Style { attributes, ..base };
        assert_spans(
            format_args!("\x1b[44;{set}mstyled\x1b[{reset}mplain"),
            &[styled.apply("styled"), base.apply("plain")],
        );
    }
    let single = Style::new().underlined().slow_blink();
    let double = Style::new()
        .add_attributes(Attributes::DOUBLE_UNDERLINED)
        .rapid_blink();
    assert_spans(
        format_args!("\x1b[4;5ma\x1b[21;6mb\x1b[4;5mc\x1b[24;25md"),
        &[
            single.apply("a"),
            double.apply("b"),
            single.apply("c"),
            Style::default().apply("d"),
        ],
    );
    for reset in ["0", "", ";"] {
        let mut buffer = Buffer::new();
        write!(
            buffer.ansi_writer(),
            "\x1b[1;2;3;4;5;7;8;9;31;44;58;5;42mstyled\x1b[{reset}mplain"
        )
        .unwrap();
        let mut spans = buffer.line(0).unwrap().spans();
        assert_ne!(spans.next().unwrap().style, Style::default());
        assert_eq!(spans.next().unwrap(), Style::default().apply("plain"));
        assert!(spans.next().is_none());
    }
}

#[test]
fn palette_and_extended_colors_reset_independently() {
    assert_spans(
        format_args!("\x1b[30;47ma\x1b[97;100mb\x1b[39mc\x1b[49md\x1b[90;107me\x1b[37;40mf"),
        &[
            Style::new()
                .fg(Color::AnsiValue(0))
                .bg(Color::AnsiValue(7))
                .apply("a"),
            Style::new()
                .fg(Color::AnsiValue(15))
                .bg(Color::AnsiValue(8))
                .apply("b"),
            Style::new().bg(Color::AnsiValue(8)).apply("c"),
            Style::default().apply("d"),
            Style::new()
                .fg(Color::AnsiValue(8))
                .bg(Color::AnsiValue(15))
                .apply("e"),
            Style::new()
                .fg(Color::AnsiValue(7))
                .bg(Color::AnsiValue(0))
                .apply("f"),
        ],
    );
    let base = Style::new().bold();
    for (code, reset) in [(38, 39), (48, 49), (58, 59)] {
        for (params, color) in [
            ("5;0", Color::AnsiValue(0)),
            ("5;255", Color::AnsiValue(255)),
            (
                "2;0;127;255",
                Color::Rgb {
                    r: 0,
                    g: 127,
                    b: 255,
                },
            ),
        ] {
            let mut styled = base;
            match code {
                38 => styled.foreground_color = Some(color),
                48 => styled.background_color = Some(color),
                58 => styled.underline_color = Some(color),
                _ => unreachable!(),
            }
            assert_spans(
                format_args!("\x1b[1;{code};{params}mcolor\x1b[{reset}mplain"),
                &[styled.apply("color"), base.apply("plain")],
            );
        }
    }
}

#[test]
fn unsupported_and_malformed_sequences_do_not_leak_into_text_or_styles() {
    let cases: &[&[u8]] = &[
        b"\x1b[999m",
        b"\x1b[?32m",
        b"\x1b[32 m",
        b"\x1b[2J\x1b[H",
        b"\x1b]0;window title\x07",
        b"\x1b]8;;https://example.com\x1b\\\x1b]8;;\x1b\\",
        b"\x1bPignored device data\x1b\\",
        b"\x1b[0;38:2:0:255:0m",
        b"\x1b[38m",
        b"\x1b[38;5m",
        b"\x1b[38;2;1;2m",
        b"\x1b[38;5;256;0m",
        b"\x1b[48;2;256;0;0;0m",
        b"\x1b[58;2;0;256;0;0m",
        b"\x1b[38;2;0;0;256;0m",
        b"\x1b[38;3;0m",
    ];
    for &sequence in cases {
        let mut buffer = Buffer::new();
        {
            let mut writer = buffer.ansi_writer();
            writer.write_all(b"\x1b[1;31mbefore").unwrap();
            writer.write_all(sequence).unwrap();
            writer.write_all(b"after").unwrap();
        }
        assert!(
            buffer.line(0).unwrap().spans().eq([Style::new()
                .fg(Color::AnsiValue(1))
                .bold()
                .apply("beforeafter"),]),
            "{sequence:?}"
        );
    }
}

#[test]
fn controls_and_invalid_utf8_preserve_styles_across_lines() {
    let cases: &[(&[u8], &[&str])] = &[
        (
            "a\r\nb\tc\x00\x07\x08\x7f\u{85}\u{9f}\n\nlast\r".as_bytes(),
            &["a", "b␉c", "", "last"],
        ),
        (b"a\xffb\xc0c", &["abc"]),
        (b"a\xe2\xffb\xf0\xffc", &["abc"]),
    ];
    let style = Style::new().fg(Color::AnsiValue(2)).bold();
    for &(input, expected) in cases {
        let mut buffer = Buffer::new();
        {
            let mut writer = buffer.ansi_writer();
            writer.write_all(b"\x1b[1;32m").unwrap();
            writer.write_all(input).unwrap();
        }
        let lossy = std::str::from_utf8(input).is_err();
        assert_eq!(buffer.lines().len(), expected.len());
        for (line, expected) in buffer.lines().zip(expected) {
            assert!(
                line.as_str()
                    .chars()
                    .filter(|&ch| !lossy || ch != '\u{fffd}')
                    .eq(expected.chars()),
                "{input:?}"
            );
            assert!(
                line.spans()
                    .eq((!line.as_str().is_empty()).then(|| style.apply(line.as_str())))
            );
        }
    }
}

#[test]
fn restarting_streams_resets_parser_and_style_but_clearing_only_resets_content() {
    let bold = Style::new().bold();
    for incomplete in [b"\x1b[".as_slice(), b"\xe7\x95", b"\x1b]0;title", b""] {
        let mut owned = Buffer::new();
        let mut caller_owned = Buffer::new();
        let mut state = AnsiState::new();
        {
            let mut writer = owned.ansi_writer();
            writer.write_all(b"\x1b[1mbold").unwrap();
            writer.write_all(incomplete).unwrap();
            writer.flush().unwrap();
        }
        caller_owned.push_ansi(b"\x1b[1mbold", &mut state);
        caller_owned.push_ansi(incomplete, &mut state);
        state = AnsiState::default();
        caller_owned.push_ansi(b"plain", &mut state);
        owned.ansi_writer().write_all(b"plain").unwrap();
        for buffer in [&owned, &caller_owned] {
            assert!(
                buffer
                    .line(0)
                    .unwrap()
                    .spans()
                    .eq([bold.apply("bold"), Style::default().apply("plain"),])
            );
        }
    }
    let mut buffer = Buffer::new();
    let mut state = AnsiState::new();
    buffer.push_ansi(b"\x1b[1mold", &mut state);
    buffer.clear();
    buffer.push_ansi(b"new", &mut state);
    assert!(buffer.line(0).unwrap().spans().eq([bold.apply("new")]));
}

#[test]
fn interleaved_streams_keep_independent_parser_and_style_state() {
    let inputs = ["\x1b[31m界\nred", "\x1b[1;34méblue"];
    let mut streams = std::array::from_fn::<_, 2, _>(|_| (Buffer::new(), AnsiState::default()));
    for i in 0..inputs.iter().map(|s| s.len()).max().unwrap() {
        for ((buffer, state), input) in streams.iter_mut().zip(inputs) {
            if let Some(byte) = input.as_bytes().get(i) {
                buffer.push_ansi(std::slice::from_ref(byte), state);
            }
        }
    }
    for ((buffer, _), (style, expected)) in streams.iter().zip([
        (Style::new().fg(Color::AnsiValue(1)), &["界", "red"][..]),
        (Style::new().fg(Color::AnsiValue(4)).bold(), &["éblue"][..]),
    ]) {
        assert!(
            buffer
                .lines()
                .map(|line| line.as_str())
                .eq(expected.iter().copied())
        );
        for line in buffer.lines() {
            assert!(line.spans().eq([style.apply(line.as_str())]));
        }
    }
}
