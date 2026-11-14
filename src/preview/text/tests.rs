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
fn writer_lifecycle_appends_unstyled_text_and_preserves_settings() {
    struct SplitCrlf;

    impl fmt::Display for SplitCrlf {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("first\t\x1b[31m\r")?;
            f.write_str("")?;
            f.write_str("\nsecond\r")
        }
    }

    let plain = ContentStyle::default();
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.set_err(true);
    buffer.set_line_numbers(true);
    buffer.set_offset(3, 4);
    buffer.push_styled_str("prefix ", red);
    {
        let mut writer = TextWriter::new(&mut buffer);
        writeln!(writer, "{SplitCrlf}").unwrap();
        for ch in "é界e\u{301}👩‍💻\r\n\r\r\nlast\r".chars() {
            writer.write_char(ch).unwrap();
            writer.write_str("").unwrap();
        }
    }
    assert!(buffer.line(4).unwrap().spans().eq([plain.apply("last␍")]));
    buffer.text_writer().write_str("").unwrap();
    buffer.text_writer().write_str("\nreopened").unwrap();
    buffer.push_styled_str(" suffix", blue);
    assert!(buffer.lines().map(|line| line.as_str()).eq([
        "prefix first␉␛[31m",
        "second",
        "é界e\u{301}👩‍💻",
        "␍",
        "last␍",
        "reopened suffix",
    ]));
    assert!(
        buffer
            .line(0)
            .unwrap()
            .spans()
            .eq([red.apply("prefix "), plain.apply("first␉␛[31m"),])
    );
    assert!(
        buffer
            .line(5)
            .unwrap()
            .spans()
            .eq([plain.apply("reopened"), blue.apply(" suffix"),])
    );
    assert!(buffer.is_err());
    assert!(buffer.line_numbers());
    assert_eq!(buffer.offset(), (3, 4));
}
