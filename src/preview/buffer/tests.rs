use std::panic::{AssertUnwindSafe, catch_unwind};

use crossterm::style::{ContentStyle, Stylize};

use super::PreviewBuffer;

#[test]
fn empty_buffers_and_clear_reuse_storage() {
    fn assert_empty(buffer: &PreviewBuffer) {
        assert_eq!(buffer.lines().len(), 1);
        assert_eq!(buffer.line(0).unwrap().as_str(), "");
        assert_eq!(buffer.line(0).unwrap().spans().count(), 0);
        assert!(buffer.line(1).is_none());
        assert!(buffer.text.is_empty());
        assert!(buffer.styles.is_empty());
        assert!(!buffer.is_err());
        assert!(!buffer.line_numbers());
        assert_eq!(buffer.offset(), (0, 0));
    }

    let style = ContentStyle::new().green();
    for mut buffer in [PreviewBuffer::new(), PreviewBuffer::default()] {
        assert_eq!(buffer.text.capacity(), 0);
        assert_eq!(buffer.lines.capacity(), 0);
        assert_eq!(buffer.styles.capacity(), 0);
        assert_empty(&buffer);
        buffer.push_str("");
        buffer.push_styled_str("", style);
        buffer.push_text("");
        buffer.push_styled_text("", style);
        assert_empty(&buffer);
        for expected in 2..=4 {
            buffer.newline();
            assert_eq!(buffer.lines().len(), expected);
            assert!(buffer.lines().all(|line| line.as_str().is_empty()));
        }
        buffer.clear();
        assert_empty(&buffer);
        buffer.push_styled_text("first\nsecond\nthird", style);
        buffer.set_err(true);
        buffer.set_line_numbers(true);
        buffer.set_offset(usize::MAX, usize::MAX);
        assert_eq!(buffer.offset(), (usize::MAX, usize::MAX));
        let capacities = (
            buffer.text.capacity(),
            buffer.lines.capacity(),
            buffer.styles.capacity(),
        );
        let pointers = (
            buffer.text.as_ptr(),
            buffer.lines.as_ptr(),
            buffer.styles.as_ptr(),
        );
        buffer.clear();
        assert_empty(&buffer);
        assert_eq!(
            capacities,
            (
                buffer.text.capacity(),
                buffer.lines.capacity(),
                buffer.styles.capacity(),
            )
        );
        buffer.push_styled_text("new\ntext", style);
        assert!(buffer.lines().map(|line| line.as_str()).eq(["new", "text"]));
        for line in buffer.lines() {
            assert!(line.spans().eq([style.apply(line.as_str())]));
        }
        assert_eq!(
            pointers,
            (
                buffer.text.as_ptr(),
                buffer.lines.as_ptr(),
                buffer.styles.as_ptr(),
            )
        );
    }
}

#[test]
fn complete_text_and_explicit_lines_preserve_content_and_iteration() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[""]),
        ("hello", &["hello"]),
        ("hello\n", &["hello", ""]),
        ("\r\n", &["", ""]),
        ("\n\r\n", &["", "", ""]),
        ("hello\r\nworld\n", &["hello", "world", ""]),
        ("\n  indented\n\nlast", &["", "  indented", "", "last"]),
        ("界\r\ne\u{301}\n👩‍💻", &["界", "e\u{301}", "👩‍💻"]),
    ];

    for style in [
        None,
        Some(ContentStyle::default()),
        Some(ContentStyle::new().green()),
    ] {
        for &(text, expected) in cases {
            let mut complete = PreviewBuffer::new();
            match style {
                Some(style) => complete.push_styled_text(text, style),
                None => complete.push_text(text),
            }
            let mut explicit = PreviewBuffer::new();
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
                assert_eq!(buffer.text, expected.concat());
                assert_eq!(buffer.lines().len(), expected.len());
                assert!(
                    buffer
                        .lines()
                        .map(|line| line.as_str())
                        .eq(expected.iter().copied())
                );
                assert!(
                    buffer
                        .lines()
                        .rev()
                        .map(|line| line.as_str())
                        .eq(expected.iter().copied().rev())
                );
                for (index, &line) in expected.iter().enumerate() {
                    let view = buffer.line(index).unwrap();
                    assert_eq!(view.as_str(), line);
                    assert!(
                        view.spans()
                            .eq((!line.is_empty()).then(|| style.unwrap_or_default().apply(line)))
                    );
                }
                assert!(buffer.line(expected.len()).is_none());
                assert!(buffer.line(usize::MAX).is_none());
                for front in 0..=expected.len() {
                    for back in 0..=expected.len() {
                        let mut lines = buffer.lines().map(|line| line.as_str());
                        let mut reference = expected.iter().copied();
                        assert_eq!(lines.nth(front), reference.nth(front));
                        assert_eq!(lines.nth_back(back), reference.nth_back(back));
                        assert_eq!(lines.len(), reference.len());
                        assert_eq!(lines.size_hint(), reference.size_hint());
                        assert!(lines.eq(reference));
                    }
                }
            }
        }
    }
}

#[test]
fn appended_styles_merge_and_spans_borrow_text_with_default_gaps() {
    let plain = ContentStyle::default();
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("prefix ");
    buffer.push_styled_str("é", red);
    buffer.push_styled_text(" first\r\n\n  界", red);
    buffer.push_styled_text("", blue);
    buffer.push_text("");
    buffer.push_styled_str(" suffix", red);
    buffer.push_str(" ");
    buffer.push_styled_str("e", red);
    buffer.push_styled_str("\u{301}", blue);
    buffer.push_styled_str("!", plain);
    buffer.push_styled_str("?", blue);
    buffer.push_text(" tail\nlast");
    assert!(buffer.lines().map(|line| line.as_str()).eq([
        "prefix é first",
        "",
        "  界 suffix e\u{301}!? tail",
        "last"
    ]));
    assert!(
        buffer
            .line(0)
            .unwrap()
            .spans()
            .eq([plain.apply("prefix "), red.apply("é first")])
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
    assert_eq!(buffer.styles.len(), 5);
    for line in buffer.lines() {
        let mut offset = 0;
        for span in line.spans() {
            let text = *span.content();
            assert_eq!(text.as_ptr(), line.as_str()[offset..].as_ptr());
            offset += text.len();
        }
        assert_eq!(offset, line.as_str().len());
    }
}

#[test]
fn sanitization_preserves_line_boundaries_and_styles() {
    let red = ContentStyle::new().red();
    let mut text: String = (0u8..=31)
        .filter(|&byte| byte != b'\n')
        .map(char::from)
        .collect();
    text.push('\x7f');
    text.extend((0x80u8..=0x9f).map(char::from));
    let discarded: String = (0x80u8..=0x9f).map(char::from).collect();
    let cases: &[(&str, &[&str])] = &[
        (&text, &["␀␁␂␃␄␅␆␇␈␉␋␌␍␎␏␐␑␒␓␔␕␖␗␘␙␚␛␜␝␞␟␡"]),
        (&discarded, &[""]),
        ("\u{85}\n\u{9f}", &["", ""]),
        (
            "界\t\u{85}e\u{301}👩‍💻\r\n\r\r\n\x1b[31m\nlast\r",
            &["界␉e\u{301}👩‍💻", "␍", "␛[31m", "last␍"],
        ),
    ];
    for style in [None, Some(red)] {
        for &(text, expected) in cases {
            let mut buffer = PreviewBuffer::new();
            match style {
                Some(style) => buffer.push_styled_text(text, style),
                None => buffer.push_text(text),
            }
            assert!(
                buffer
                    .lines()
                    .map(|line| line.as_str())
                    .eq(expected.iter().copied())
            );
            for (line, &text) in buffer.lines().zip(expected) {
                assert!(
                    line.spans()
                        .eq((!text.is_empty()).then(|| style.unwrap_or_default().apply(text)))
                );
                assert!(!line.as_str().chars().any(char::is_control));
            }
            if expected.iter().all(|line| line.is_empty()) {
                assert!(buffer.text.is_empty());
                assert!(buffer.styles.is_empty());
            }
        }
    }
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_str("prefix ", red);
    buffer.push_styled_text("\t\r", red);
    buffer.push_styled_text("\n\u{80}\u{9f}", blue);
    buffer.push_styled_str("suffix", blue);
    assert_eq!(buffer.lines().len(), 2);
    assert!(buffer.line(0).unwrap().spans().eq([red.apply("prefix ␉␍")]));
    assert!(buffer.line(1).unwrap().spans().eq([blue.apply("suffix")]));
}

#[test]
fn line_helpers_finish_the_current_line() {
    let red = ContentStyle::new().red();
    let mut buffer = PreviewBuffer::new();
    buffer.push_line("");
    buffer.push_styled_str("prefix ", red);
    buffer.push_line("plain");
    buffer.push_styled_str("same ", red);
    buffer.push_styled_line("style", red);
    buffer.push_styled_line("", red);
    assert!(buffer.lines().map(|line| line.as_str()).eq([
        "",
        "prefix plain",
        "same style",
        "",
        ""
    ]));
    assert!(
        buffer
            .line(1)
            .unwrap()
            .spans()
            .eq([red.apply("prefix "), ContentStyle::default().apply("plain")])
    );
    assert!(
        buffer
            .line(2)
            .unwrap()
            .spans()
            .eq([red.apply("same style")])
    );
    assert_eq!(buffer.styles.len(), 2);

    for text in ["\n", "text\r\n", "first\nsecond"] {
        for styled in [false, true] {
            let result = catch_unwind(AssertUnwindSafe(|| {
                if styled {
                    buffer.push_styled_line(text, red);
                } else {
                    buffer.push_line(text);
                }
            }));
            assert!(result.is_err(), "accepted {text:?}");
            assert_eq!(buffer.lines().len(), 5);
            assert_eq!(buffer.text, "prefix plainsame style");
            assert_eq!(buffer.styles.len(), 2);
        }
    }
}

#[test]
fn chunk_boundaries_do_not_change_lines_or_styles() {
    let text = "é界e\u{301}👩‍💻";
    let style = ContentStyle::new().blue().bold();

    for boundary in (0..=text.len()).filter(|&index| text.is_char_boundary(index)) {
        let mut buffer = PreviewBuffer::new();
        buffer.push_styled_str(&text[..boundary], style);
        buffer.push_styled_str("", ContentStyle::new().red());
        buffer.push_str("");
        buffer.push_styled_str(&text[boundary..], style);
        assert_eq!(buffer.text, text);
        assert_eq!(buffer.lines().len(), 1);
        assert!(buffer.line(0).unwrap().spans().eq([style.apply(text)]));

        buffer.newline();
        buffer.newline();
        buffer.push_styled_str(text, style);
        buffer.newline();
        assert!(
            buffer
                .lines()
                .map(|line| line.as_str())
                .eq([text, "", text, ""])
        );
        assert_eq!(buffer.line(1).unwrap().spans().count(), 0);
        assert!(buffer.line(2).unwrap().spans().eq([style.apply(text)]));
        assert_eq!(buffer.line(3).unwrap().spans().count(), 0);
        assert_eq!(buffer.styles.len(), 2);
    }

    let mut buffer = PreviewBuffer::new();
    for ch in text.chars() {
        buffer.push_str(ch.encode_utf8(&mut [0; 4]));
    }
    assert_eq!(buffer.text, text);
    assert!(buffer.lines().map(|line| line.as_str()).eq([text]));
    assert!(buffer.styles.is_empty());
}

#[test]
fn segment_writes_reject_line_breaks_without_mutation() {
    let red = ContentStyle::new().red();
    for style in [None, Some(ContentStyle::default()), Some(red)] {
        for populated in [false, true] {
            let mut buffer = PreviewBuffer::new();
            if populated {
                buffer.push_styled_str("previous", red);
                buffer.newline();
                buffer.push_str("prefix ");
                buffer.push_styled_str("current", red);
            }
            for text in ["\n", "\r\n", "before\nafter", "界\nafter", "last\n"] {
                let result = catch_unwind(AssertUnwindSafe(|| match style {
                    Some(style) => buffer.push_styled_str(text, style),
                    None => buffer.push_str(text),
                }));
                assert!(result.is_err(), "accepted {text:?}");
                if populated {
                    assert_eq!(buffer.text, "previousprefix current");
                    assert_eq!(buffer.lines().len(), 2);
                    assert_eq!(buffer.styles.len(), 2);
                    assert!(buffer.line(0).unwrap().spans().eq([red.apply("previous")]));
                    assert!(buffer.line(1).unwrap().spans().eq([
                        ContentStyle::default().apply("prefix "),
                        red.apply("current")
                    ]));
                } else {
                    assert!(buffer.text.is_empty());
                    assert_eq!(buffer.lines().len(), 1);
                    assert!(buffer.styles.is_empty());
                }
            }
            buffer.push_styled_str("next", red);
            assert_eq!(
                buffer.lines().next_back().unwrap().as_str(),
                if populated {
                    "prefix currentnext"
                } else {
                    "next"
                }
            );
        }
    }
}

#[test]
fn indexed_lines_and_skipping_access_only_their_styles() {
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    for index in 0..1100 {
        buffer.push_str(&format!("{index}: "));
        buffer.push_styled_str("red", red);
        buffer.push_styled_str(" blue", blue);
        buffer.newline();
    }

    for (index, view) in (1000..1050).zip(buffer.lines().skip(1000).take(50)) {
        let direct = buffer.line(index).unwrap();
        assert_eq!(view.as_str(), format!("{index}: red blue"));
        assert_eq!(view.as_str().as_ptr(), direct.as_str().as_ptr());
        assert_eq!(view.styles.as_ptr(), buffer.styles[index * 2..].as_ptr());
        assert_eq!(view.styles.len(), 2);
        assert!(view.spans().eq([
            ContentStyle::default().apply(view.as_str().split_at(6).0),
            red.apply("red"),
            blue.apply(" blue"),
        ]));
    }

    let mut lines = buffer.lines();
    assert_eq!(lines.nth(1000).unwrap().as_str(), "1000: red blue");
    assert_eq!(lines.len(), 100);
    assert_eq!(lines.next_back().unwrap().as_str(), "");
    assert_eq!(lines.nth_back(49).unwrap().as_str(), "1050: red blue");
    assert_eq!(lines.len(), 49);
    assert!(lines.nth(usize::MAX).is_none());
    assert_eq!(lines.len(), 0);

    let mut lines = buffer.lines();
    assert!(lines.nth_back(usize::MAX).is_none());
    assert_eq!(lines.len(), 0);
    assert!(lines.next().is_none());
    assert!(lines.next_back().is_none());
}
