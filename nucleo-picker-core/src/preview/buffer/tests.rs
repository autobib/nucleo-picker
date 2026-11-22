use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::draw::{Color, Style};

use super::Buffer;

fn assert_lines(buffer: &Buffer, expected: &[&str]) {
    assert!(
        buffer
            .lines()
            .map(|line| line.as_str())
            .eq(expected.iter().copied())
    );
}

#[test]
fn text_and_segment_input_preserve_lines_and_styles() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[""]),
        ("hello", &["hello"]),
        ("hello\n", &["hello", ""]),
        ("\n\r\n", &["", "", ""]),
        ("\n  indented\n\nlast", &["", "  indented", "", "last"]),
        ("界\r\ne\u{301}\n👩‍💻", &["界", "e\u{301}", "👩‍💻"]),
    ];
    for style in [None, Some(Style::new().fg(Color::Green))] {
        for &(text, expected) in cases {
            let mut complete = Buffer::new();
            match style {
                Some(style) => complete.push_styled_text(text, style),
                None => complete.push_text(text),
            }
            let mut explicit = Buffer::new();
            for (index, &line) in expected.iter().enumerate() {
                if index > 0 {
                    explicit.newline();
                }
                match style {
                    Some(style) => explicit.push_styled_str(line, style),
                    None => explicit.push_str(line),
                }
            }
            for buffer in [complete, explicit] {
                assert_lines(&buffer, expected);
                for (index, &text) in expected.iter().enumerate() {
                    let line = buffer.line(index).unwrap();
                    assert_eq!(line.as_str(), text);
                    assert!(
                        line.spans()
                            .eq((!text.is_empty()).then(|| style.unwrap_or_default().apply(text)))
                    );
                }
                assert!(buffer.line(expected.len()).is_none());
            }
        }
    }
}

#[test]
fn appended_styles_preserve_unicode_and_default_gaps() {
    let plain = Style::default();
    let red = Style::new().fg(Color::Red);
    let blue = Style::new().fg(Color::Blue);
    let mut buffer = Buffer::new();
    buffer.push_str("prefix ");
    buffer.push_styled_str("é", red);
    buffer.push_styled_text(" first\r\n\n  界", red);
    buffer.push_styled_text("", blue);
    buffer.push_styled_str(" suffix", red);
    buffer.push_str(" ");
    buffer.push_styled_str("e", red);
    buffer.push_styled_str("\u{301}", blue);
    buffer.push_styled_str("!", plain);
    buffer.push_styled_str("?", blue);
    buffer.push_text(" tail\nlast");
    assert_lines(
        &buffer,
        &["prefix é first", "", "  界 suffix e\u{301}!? tail", "last"],
    );
    assert!(
        buffer
            .line(0)
            .unwrap()
            .spans()
            .eq([plain.apply("prefix "), red.apply("é first"),])
    );
    assert_eq!(buffer.line(1).unwrap().spans().count(), 0);
    assert!(buffer.line(2).unwrap().spans().eq([
        red.apply("  界 suffix"),
        plain.apply(" "),
        red.apply("e"),
        blue.apply("\u{301}"),
        plain.apply("!"),
        blue.apply("?"),
        plain.apply(" tail"),
    ]));
    assert!(buffer.line(3).unwrap().spans().eq([plain.apply("last")]));

    let text = "é界e\u{301}👩‍💻";
    for boundary in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
        buffer.clear();
        buffer.push_styled_str(&text[..boundary], red);
        buffer.push_styled_str("", blue);
        buffer.push_str("");
        buffer.push_styled_str(&text[boundary..], red);
        assert!(buffer.line(0).unwrap().spans().eq([red.apply(text)]));
    }
}

#[test]
fn sanitization_preserves_line_boundaries_and_styles() {
    let controls: String = (0u8..=31)
        .filter(|&byte| byte != b'\n')
        .chain([127])
        .map(char::from)
        .collect();
    let discarded: String = (0x80u8..=0x9f).map(char::from).collect();
    let cases: &[(&str, &[&str])] = &[
        (&controls, &["␀␁␂␃␄␅␆␇␈␉␋␌␍␎␏␐␑␒␓␔␕␖␗␘␙␚␛␜␝␞␟␡"]),
        (&discarded, &[""]),
        ("\u{85}\n\u{9f}", &["", ""]),
        (
            "界\t\u{85}e\u{301}👩‍💻\r\n\r\r\n\x1b[31m\nlast\r",
            &["界␉e\u{301}👩‍💻", "␍", "␛[31m", "last␍"],
        ),
    ];
    for style in [Style::default(), Style::new().fg(Color::Red)] {
        for &(text, expected) in cases {
            let mut buffer = Buffer::new();
            buffer.push_styled_text(text, style);
            assert_lines(&buffer, expected);
            for (line, &text) in buffer.lines().zip(expected) {
                assert!(
                    line.spans()
                        .eq((!text.is_empty()).then(|| style.apply(text)))
                );
            }
        }
    }
}

#[test]
fn line_helpers_finish_the_current_line_and_clear_removes_styles() {
    let red = Style::new().fg(Color::Red);
    let mut buffer = Buffer::new();
    buffer.push_line("");
    buffer.push_styled_str("prefix ", red);
    buffer.push_line("plain");
    buffer.push_styled_str("same ", red);
    buffer.push_styled_line("style", red);
    buffer.push_styled_line("", red);
    assert_lines(&buffer, &["", "prefix plain", "same style", "", ""]);
    assert!(
        buffer
            .line(1)
            .unwrap()
            .spans()
            .eq([red.apply("prefix "), Style::default().apply("plain"),])
    );
    assert!(
        buffer
            .line(2)
            .unwrap()
            .spans()
            .eq([red.apply("same style")])
    );

    buffer.clear();
    assert_lines(&buffer, &[""]);
    assert_eq!(buffer.line(0).unwrap().spans().count(), 0);
    buffer.push_text("new\ntext");
    assert_lines(&buffer, &["new", "text"]);
    for line in buffer.lines() {
        assert!(line.spans().eq([Style::default().apply(line.as_str())]));
    }
}

#[test]
fn segment_and_line_writes_reject_newlines_without_mutation() {
    let red = Style::new().fg(Color::Red);
    let writes: [fn(&mut Buffer, &str); 4] = [
        Buffer::push_str,
        Buffer::push_line,
        |buffer, text| buffer.push_styled_str(text, Style::new().fg(Color::Red)),
        |buffer, text| buffer.push_styled_line(text, Style::new().fg(Color::Red)),
    ];
    for write in writes {
        let mut buffer = Buffer::new();
        buffer.push_styled_line("previous", red);
        buffer.push_str("prefix ");
        buffer.push_styled_str("current", red);
        for text in ["\n", "\r\n", "界\nafter", "last\n"] {
            assert!(catch_unwind(AssertUnwindSafe(|| write(&mut buffer, text))).is_err());
            assert_lines(&buffer, &["previous", "prefix current"]);
            assert!(buffer.line(0).unwrap().spans().eq([red.apply("previous")]));
            assert!(
                buffer
                    .line(1)
                    .unwrap()
                    .spans()
                    .eq([Style::default().apply("prefix "), red.apply("current"),])
            );
        }
        buffer.push_styled_str("next", red);
        assert_eq!(buffer.line(1).unwrap().as_str(), "prefix currentnext");
    }
}

#[test]
fn line_iteration_supports_mixed_direction_skips_and_out_of_bounds() {
    let mut buffer = Buffer::new();
    buffer.push_text("zero\né\n\n界\nlast\n");
    assert!(
        buffer
            .lines()
            .rev()
            .map(|line| line.as_str())
            .eq(["", "last", "界", "", "é", "zero"])
    );
    let mut lines = buffer.lines();
    assert_eq!(lines.nth(1).unwrap().as_str(), "é");
    assert_eq!(lines.next_back().unwrap().as_str(), "");
    assert_eq!(lines.nth_back(1).unwrap().as_str(), "界");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines.next().unwrap().as_str(), "");
    assert!(lines.next_back().is_none());
    assert!(buffer.line(usize::MAX).is_none());
    for backwards in [false, true] {
        let mut lines = buffer.lines();
        assert!(if backwards {
            lines.nth_back(usize::MAX).is_none()
        } else {
            lines.nth(usize::MAX).is_none()
        });
        assert_eq!(lines.len(), 0);
    }
}
