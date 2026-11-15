use std::fmt::Write;

use crossterm::style::{ContentStyle, Stylize};

use crate::preview::{Buffer, TextWriter};

#[test]
fn chunk_boundaries_preserve_unicode_controls_and_crlf() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[""]),
        ("plain\n\n", &["plain", "", ""]),
        ("\r", &["␍"]),
        ("\r\n", &["", ""]),
        ("\r\r\n", &["␍", ""]),
        ("\r\r\r", &["␍␍␍"]),
        ("\r\u{85}\n\r\t\n", &["␍", "␍␉", ""]),
        (
            "é界e\u{301}👩‍💻\r\n\t\x1b[31mend\r",
            &["é界e\u{301}👩‍💻", "␉␛[31mend␍"],
        ),
    ];
    for &(input, expected) in cases {
        for boundary in (0..=input.len()).filter(|&i| input.is_char_boundary(i)) {
            let mut buffer = Buffer::new();
            {
                let mut writer = buffer.text_writer();
                writer.write_str(&input[..boundary]).unwrap();
                writer.write_str("").unwrap();
                writer.write_str(&input[boundary..]).unwrap();
            }
            assert!(
                buffer
                    .lines()
                    .map(|line| line.as_str())
                    .eq(expected.iter().copied()),
                "{input:?}, boundary {boundary}"
            );
            for (line, &text) in buffer.lines().zip(expected) {
                assert!(
                    line.spans()
                        .eq((!text.is_empty()).then(|| ContentStyle::default().apply(text)))
                );
            }
        }
    }
}

#[test]
fn formatted_and_character_writes_append_plain_text_and_flush_pending_cr_on_drop() {
    let plain = ContentStyle::default();
    let red = ContentStyle::new().red();
    let mut buffer = Buffer::new();
    buffer.push_styled_str("prefix ", red);
    {
        let mut writer = TextWriter::new(&mut buffer);
        let label = "preview";
        write!(writer, "{label}: ").unwrap();
        for ch in "é界e\u{301}👩‍💻\r\nlast\r".chars() {
            writer.write_char(ch).unwrap();
        }
    }
    buffer.text_writer().write_str("\nreopened").unwrap();
    buffer.push_styled_str(" suffix", red);
    assert!(buffer.lines().map(|line| line.as_str()).eq([
        "prefix preview: é界e\u{301}👩‍💻",
        "last␍",
        "reopened suffix",
    ]));
    assert!(
        buffer
            .line(0)
            .unwrap()
            .spans()
            .eq([red.apply("prefix "), plain.apply("preview: é界e\u{301}👩‍💻"),])
    );
    assert!(buffer.line(1).unwrap().spans().eq([plain.apply("last␍")]));
    assert!(
        buffer
            .line(2)
            .unwrap()
            .spans()
            .eq([plain.apply("reopened"), red.apply(" suffix"),])
    );
}
