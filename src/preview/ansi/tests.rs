use std::{
    fmt,
    io::{self, Write},
};

use crossterm::style::{Attribute, ContentStyle, Stylize};

use crate::preview::{AnsiState, AnsiWriter, PreviewBuffer};

use super::Color;

#[test]
fn writes_preserve_text_and_styles_at_every_byte_boundary() {
    let input = "\x1b[1;31mé界e\u{301}👩‍💻\x1b[0m!\r\n\t\x1b[38;2;1;2;3mend".as_bytes();
    for boundary in 0..=input.len() {
        let mut buffer = PreviewBuffer::new();
        buffer.push_str("prefix ");
        {
            let mut writer: AnsiWriter<'_> = buffer.ansi_writer();
            assert_eq!(writer.write(&input[..boundary]).unwrap(), boundary);
            assert_eq!(writer.write(b"").unwrap(), 0);
            writer.flush().unwrap();
            writer.write_all(&input[boundary..]).unwrap();
        }
        assert_eq!(buffer.lines().len(), 2);
        assert!(
            buffer.line(0).unwrap().spans().eq([
                ContentStyle::default().apply("prefix "),
                ContentStyle::new()
                    .with(Color::AnsiValue(1))
                    .bold()
                    .apply("é界e\u{301}👩‍💻"),
                ContentStyle::default().apply("!"),
            ])
        );
        assert!(
            buffer.line(1).unwrap().spans().eq([
                ContentStyle::default().apply("␉"),
                ContentStyle::new()
                    .with(Color::Rgb { r: 1, g: 2, b: 3 })
                    .apply("end"),
            ])
        );
    }

    let mut buffer = PreviewBuffer::new();
    {
        let mut writer = buffer.ansi_writer();
        for byte in input {
            writer.write_all(std::slice::from_ref(byte)).unwrap();
            writer.flush().unwrap();
        }
    }
    assert!(
        buffer
            .lines()
            .map(|line| line.as_str())
            .eq(["é界e\u{301}👩‍💻!", "␉end"])
    );
    assert_eq!(buffer.line(0).unwrap().spans().count(), 2);
    assert_eq!(buffer.line(1).unwrap().spans().count(), 2);
}

#[test]
fn formatted_writes_preserve_text_and_styles() {
    let mut buffer = PreviewBuffer::new();
    {
        let writer: &mut dyn fmt::Write = &mut buffer.ansi_writer();
        let color = 31;
        write!(writer, "\x1b[1;{color}m").unwrap();
        for ch in "é界e\u{301}👩‍💻".chars() {
            writer.write_char(ch).unwrap();
        }
        writeln!(writer, "\x1b[0m!").unwrap();
        writer.write_str("\tlast").unwrap();
    }
    assert_eq!(buffer.lines().len(), 2);
    assert!(
        buffer.line(0).unwrap().spans().eq([
            ContentStyle::new()
                .with(Color::AnsiValue(1))
                .bold()
                .apply("é界e\u{301}👩‍💻"),
            ContentStyle::default().apply("!"),
        ])
    );
    assert!(
        buffer
            .line(1)
            .unwrap()
            .spans()
            .eq([ContentStyle::default().apply("␉last")])
    );
}

#[test]
fn byte_and_text_writes_share_parser_state() {
    let input = "\x1b[1;31mé界e\u{301}👩‍💻\x1b[0m!\r\n\t\x1b[38;2;1;2;3mend";
    let mut expected = PreviewBuffer::new();
    expected.ansi_writer().write_all(input.as_bytes()).unwrap();

    for boundary in (0..=input.len()).filter(|&i| input.is_char_boundary(i)) {
        for text_first in [false, true] {
            let mut buffer = PreviewBuffer::new();
            {
                let mut writer = buffer.ansi_writer();
                let (first, second) = input.split_at(boundary);
                if text_first {
                    fmt::Write::write_str(&mut writer, first).unwrap();
                } else {
                    writer.write_all(first.as_bytes()).unwrap();
                }
                fmt::Write::write_str(&mut writer, "").unwrap();
                assert_eq!(writer.write(b"").unwrap(), 0);
                writer.flush().unwrap();
                if text_first {
                    writer.write_all(second.as_bytes()).unwrap();
                } else {
                    fmt::Write::write_str(&mut writer, second).unwrap();
                }
            }
            assert_eq!(buffer.lines().len(), expected.lines().len());
            for (actual, expected) in buffer.lines().zip(expected.lines()) {
                assert!(
                    actual.spans().eq(expected.spans()),
                    "boundary {boundary}, text first: {text_first}"
                );
            }
        }
    }
}

#[test]
fn attributes_and_selective_resets_preserve_colors() {
    use Attribute::*;

    let cases: &[(&str, u8, &[Attribute])] = &[
        ("1", 22, &[Bold]),
        ("2", 22, &[Dim]),
        ("1;2", 22, &[Bold, Dim]),
        ("3", 23, &[Italic]),
        ("4", 24, &[Underlined]),
        ("21", 24, &[DoubleUnderlined]),
        ("5", 25, &[SlowBlink]),
        ("6", 25, &[RapidBlink]),
        ("7", 27, &[Reverse]),
        ("8", 28, &[Hidden]),
        ("9", 29, &[CrossedOut]),
    ];
    let base = ContentStyle::new().on(Color::AnsiValue(4));
    for &(set, reset, attributes) in cases {
        let mut buffer = PreviewBuffer::new();
        write!(
            buffer.ansi_writer(),
            "\x1b[44;{set}mstyled\x1b[{reset}mplain"
        )
        .unwrap();
        let styled = ContentStyle {
            attributes: attributes.into(),
            ..base
        };
        assert!(
            buffer
                .line(0)
                .unwrap()
                .spans()
                .eq([styled.apply("styled"), base.apply("plain"),]),
            "{set}"
        );
    }
}

#[test]
fn underline_and_blink_modes_replace_each_other() {
    let mut buffer = PreviewBuffer::new();
    buffer
        .ansi_writer()
        .write_all(b"\x1b[4;5ma\x1b[21;6mb\x1b[4;5mc\x1b[24;25md")
        .unwrap();
    let single = ContentStyle::new().underlined().slow_blink();
    let double = ContentStyle::new()
        .attribute(Attribute::DoubleUnderlined)
        .rapid_blink();
    assert!(buffer.line(0).unwrap().spans().eq([
        single.apply("a"),
        double.apply("b"),
        single.apply("c"),
        ContentStyle::default().apply("d"),
    ]));
}

#[test]
fn standard_and_bright_colors_reset_independently() {
    let mut buffer = PreviewBuffer::new();
    buffer
        .ansi_writer()
        .write_all(b"\x1b[30;47ma\x1b[97;100mb\x1b[39mc\x1b[49md\x1b[90;107me\x1b[37;40mf")
        .unwrap();
    assert!(
        buffer.line(0).unwrap().spans().eq([
            ContentStyle::new()
                .with(Color::AnsiValue(0))
                .on(Color::AnsiValue(7))
                .apply("a"),
            ContentStyle::new()
                .with(Color::AnsiValue(15))
                .on(Color::AnsiValue(8))
                .apply("b"),
            ContentStyle::new().on(Color::AnsiValue(8)).apply("c"),
            ContentStyle::default().apply("d"),
            ContentStyle::new()
                .with(Color::AnsiValue(8))
                .on(Color::AnsiValue(15))
                .apply("e"),
            ContentStyle::new()
                .with(Color::AnsiValue(7))
                .on(Color::AnsiValue(0))
                .apply("f"),
        ])
    );
}

#[test]
fn extended_colors_and_resets_preserve_other_styles() {
    let base = ContentStyle::new().bold();
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
            let mut buffer = PreviewBuffer::new();
            write!(
                buffer.ansi_writer(),
                "\x1b[1;{code};{params}mcolor\x1b[{reset}mplain"
            )
            .unwrap();
            let mut styled = base;
            match code {
                38 => styled.foreground_color = Some(color),
                48 => styled.background_color = Some(color),
                58 => styled.underline_color = Some(color),
                _ => unreachable!(),
            }
            assert!(
                buffer
                    .line(0)
                    .unwrap()
                    .spans()
                    .eq([styled.apply("color"), base.apply("plain"),])
            );
        }
    }
}

#[test]
fn full_resets_clear_attributes_and_all_colors() {
    for reset in ["0", "", ";"] {
        let mut buffer = PreviewBuffer::new();
        write!(
            buffer.ansi_writer(),
            "\x1b[1;2;3;4;5;7;8;9;31;44;58;5;42mstyled\x1b[{reset}mplain"
        )
        .unwrap();
        let mut spans = buffer.line(0).unwrap().spans();
        assert_ne!(*spans.next().unwrap().style(), ContentStyle::default());
        assert_eq!(
            spans.next().unwrap(),
            ContentStyle::default().apply("plain")
        );
        assert!(spans.next().is_none());
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
        b"\x1b[0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0m",
    ];
    for &sequence in cases {
        let mut buffer = PreviewBuffer::new();
        {
            let mut writer = buffer.ansi_writer();
            writer.write_all(b"\x1b[1;31mbefore").unwrap();
            writer.write_all(sequence).unwrap();
            writer.write_all(b"after").unwrap();
        }
        assert!(
            buffer.line(0).unwrap().spans().eq([ContentStyle::new()
                .with(Color::AnsiValue(1))
                .bold()
                .apply("beforeafter"),]),
            "{sequence:?}"
        );
    }
}

#[test]
fn controls_are_sanitized_and_styles_cross_line_breaks() {
    let mut buffer = PreviewBuffer::new();
    buffer
        .ansi_writer()
        .write_all("\x1b[32ma\r\nb\tc\x00\x07\x08\x7f\u{85}\u{9f}\n\nlast\r".as_bytes())
        .unwrap();
    assert!(
        buffer
            .lines()
            .map(|line| line.as_str())
            .eq(["a", "b␉c", "", "last"])
    );
    for line in buffer.lines() {
        assert!(!line.as_str().chars().any(char::is_control));
        if !line.as_str().is_empty() {
            assert!(
                line.spans().eq([ContentStyle::new()
                    .with(Color::AnsiValue(2))
                    .apply(line.as_str()),])
            );
        }
    }
}

#[test]
fn new_writers_start_fresh_without_clearing_existing_content() {
    for incomplete in [b"\x1b[".as_slice(), b"\xe7\x95", b"\x1b]0;title", b""] {
        let mut buffer = PreviewBuffer::new();
        {
            let mut writer = buffer.ansi_writer();
            writer.write_all(b"\x1b[1mbold").unwrap();
            writer.write_all(incomplete).unwrap();
            writer.flush().unwrap();
        }
        assert_eq!(buffer.line(0).unwrap().as_str(), "bold");
        let copied = io::copy(&mut b"plain".as_slice(), &mut buffer.ansi_writer()).unwrap();
        assert_eq!(copied, 5);
        assert!(buffer.line(0).unwrap().spans().eq([
            ContentStyle::new().bold().apply("bold"),
            ContentStyle::default().apply("plain"),
        ]));
        buffer.clear();
        buffer.ansi_writer().write_all(b"new").unwrap();
        assert!(
            buffer
                .line(0)
                .unwrap()
                .spans()
                .eq([ContentStyle::default().apply("new")])
        );
    }
}

#[test]
fn invalid_utf8_follows_parser_recovery_without_losing_style() {
    for (input, expected) in [
        (b"a\xffb\xc0c".as_slice(), "abc"),
        (b"a\xe2\xffb\xf0\xffc", "a�b�c"),
    ] {
        let mut buffer = PreviewBuffer::new();
        {
            let mut writer = buffer.ansi_writer();
            writer.write_all(b"\x1b[1m").unwrap();
            writer.write_all(input).unwrap();
        }
        assert_eq!(buffer.line(0).unwrap().as_str(), expected);
        assert!(
            buffer
                .line(0)
                .unwrap()
                .spans()
                .eq([ContentStyle::new().bold().apply(expected),])
        );
    }
}

#[test]
fn caller_owned_state_preserves_every_byte_boundary() {
    let input = "\x1b[1;31mé界\x1b[0m!\n\x1b[32mlast".as_bytes();
    for boundary in 0..=input.len() {
        let mut buffer = PreviewBuffer::new();
        let mut state = AnsiState::new();
        buffer.push_ansi(&input[..boundary], &mut state);
        buffer.push_ansi(b"", &mut state);
        let (mut buffer, mut state) = (Box::new(buffer), Box::new(state));
        buffer.push_ansi(&input[boundary..], &mut state);
        assert!(
            buffer.line(0).unwrap().spans().eq([
                ContentStyle::new()
                    .with(Color::AnsiValue(1))
                    .bold()
                    .apply("é界"),
                ContentStyle::default().apply("!"),
            ])
        );
        assert!(
            buffer
                .line(1)
                .unwrap()
                .spans()
                .eq([ContentStyle::new().with(Color::AnsiValue(2)).apply("last"),])
        );
    }
}

#[test]
fn interleaved_streams_keep_independent_parser_and_style_state() {
    let inputs = ["\x1b[31m界\nred", "\x1b[1;34méblue"];
    let mut streams =
        std::array::from_fn::<_, 2, _>(|_| (PreviewBuffer::new(), AnsiState::default()));
    for i in 0..inputs.iter().map(|s| s.len()).max().unwrap() {
        for ((buffer, state), input) in streams.iter_mut().zip(inputs) {
            if let Some(byte) = input.as_bytes().get(i) {
                buffer.push_ansi(std::slice::from_ref(byte), state);
            }
        }
    }
    assert!(
        streams[0]
            .0
            .line(0)
            .unwrap()
            .spans()
            .eq([ContentStyle::new().with(Color::AnsiValue(1)).apply("界"),])
    );
    assert!(
        streams[0]
            .0
            .line(1)
            .unwrap()
            .spans()
            .eq([ContentStyle::new().with(Color::AnsiValue(1)).apply("red"),])
    );
    assert!(
        streams[1]
            .0
            .line(0)
            .unwrap()
            .spans()
            .eq([ContentStyle::new()
                .with(Color::AnsiValue(4))
                .bold()
                .apply("éblue"),])
    );
}

#[test]
fn resetting_state_discards_partial_sequences_and_styles() {
    for incomplete in [b"\x1b[".as_slice(), b"\xe7\x95", b"\x1b]0;title", b""] {
        let mut buffer = PreviewBuffer::new();
        let mut state = AnsiState::new();
        buffer.push_ansi(b"\x1b[1mbold", &mut state);
        buffer.push_ansi(incomplete, &mut state);
        state = AnsiState::default();
        buffer.push_ansi(b"plain", &mut state);
        assert!(buffer.line(0).unwrap().spans().eq([
            ContentStyle::new().bold().apply("bold"),
            ContentStyle::default().apply("plain"),
        ]));
    }
    let mut buffer = PreviewBuffer::new();
    let mut state = AnsiState::new();
    buffer.push_ansi(b"\x1b[1mbold", &mut state);
    buffer.clear();
    buffer.push_ansi(b"still bold", &mut state);
    assert!(
        buffer
            .line(0)
            .unwrap()
            .spans()
            .eq([ContentStyle::new().bold().apply("still bold"),])
    );
}
