use std::fmt::{self, Write};

use crossterm::style::{ContentStyle, Stylize};

use crate::preview::{PreviewBuffer, TextWriter};

#[test]
fn writes_match_push_text_at_every_character_boundary() {
    let controls: String = (0u8..=0x9f).map(char::from).collect();
    for input in [
        "",
        "plain text",
        "\n\n",
        "\r",
        "\r\n",
        "\r\r\n",
        "\r\r\r",
        "first\r\n\n  last\r",
        "\r\u{85}\n\r\t\n",
        "é界e\u{301}👩‍💻\r\n\t\x1b[31mend\r",
        &controls,
    ] {
        let mut expected = PreviewBuffer::new();
        expected.push_text(input);
        for first in (0..=input.len()).filter(|&i| input.is_char_boundary(i)) {
            for second in (first..=input.len()).filter(|&i| input.is_char_boundary(i)) {
                let mut buffer = PreviewBuffer::new();
                {
                    let mut writer: TextWriter<'_> = buffer.text_writer();
                    for chunk in [&input[..first], &input[first..second], &input[second..]] {
                        writer.write_str(chunk).unwrap();
                        writer.write_str("").unwrap();
                    }
                }
                assert!(
                    buffer
                        .lines()
                        .map(|line| line.as_str())
                        .eq(expected.lines().map(|line| line.as_str())),
                    "{input:?}, boundaries {first}, {second}"
                );
                for line in buffer.lines() {
                    assert!(
                        line.spans()
                            .all(|span| *span.style() == ContentStyle::default())
                    );
                }
            }
        }
    }
}

#[test]
fn formatted_and_character_writes_preserve_crlf() {
    struct SplitCrlf;

    impl fmt::Display for SplitCrlf {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("first\r")?;
            f.write_str("")?;
            f.write_str("\nsecond\r")
        }
    }

    let mut buffer = PreviewBuffer::new();
    {
        let mut writer = TextWriter::new(&mut buffer);
        writeln!(writer, "{SplitCrlf}").unwrap();
        for ch in "é界e\u{301}👩‍💻\r\n\r\r\nlast\r".chars() {
            writer.write_char(ch).unwrap();
            writer.write_str("").unwrap();
        }
    }
    assert!(buffer.lines().map(|line| line.as_str()).eq([
        "first",
        "second",
        "é界e\u{301}👩‍💻",
        "␍",
        "last␍",
    ]));
}

#[test]
fn writes_append_unstyled_text_and_preserve_buffer_settings() {
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.set_err(true);
    buffer.set_line_numbers(true);
    buffer.push_styled_str("prefix ", red);
    {
        let mut writer = buffer.text_writer();
        writer.write_str("plain\t\x1b[31m\r").unwrap();
        writer.write_str("\nlast\r").unwrap();
    }
    buffer.push_styled_str(" suffix", blue);
    assert!(buffer.line(0).unwrap().spans().eq([
        red.apply("prefix "),
        ContentStyle::default().apply("plain␉␛[31m"),
    ]));
    assert!(buffer.line(1).unwrap().spans().eq([
        ContentStyle::default().apply("last␍"),
        blue.apply(" suffix"),
    ]));
    assert_eq!(buffer.lines().len(), 2);
    assert!(buffer.is_err());
    assert!(buffer.line_numbers());
}

#[test]
fn dropping_a_writer_finishes_its_stream() {
    let mut buffer = PreviewBuffer::new();
    buffer.text_writer().write_str("first\r").unwrap();
    assert_eq!(buffer.line(0).unwrap().as_str(), "first␍");
    buffer.text_writer().write_str("").unwrap();
    buffer.text_writer().write_str("\nsecond").unwrap();
    assert!(
        buffer
            .lines()
            .map(|line| line.as_str())
            .eq(["first␍", "second"])
    );
}
