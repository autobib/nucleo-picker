use std::panic::{AssertUnwindSafe, catch_unwind};

use crossterm::style::{ContentStyle, Stylize};

use super::PreviewBuffer;

#[test]
fn empty_buffers_always_have_a_current_line() {
    let style = ContentStyle::new().red();
    for mut buffer in [PreviewBuffer::new(), PreviewBuffer::default()] {
        for _ in 0..2 {
            assert_eq!(buffer.lines().len(), 1);
            assert_eq!(buffer.line(0).unwrap().as_str(), "");
            assert_eq!(buffer.line(0).unwrap().spans().count(), 0);
            assert!(buffer.line(1).is_none());
            assert!(!buffer.line_numbers());

            buffer.push_str("");
            buffer.push_styled_str("", style);
            buffer.push_text("");
            buffer.push_styled_text("", style);
            assert_eq!(buffer.lines().len(), 1);
            assert_eq!(buffer.line(0).unwrap().spans().count(), 0);

            for expected in 2..=4 {
                buffer.newline();
                assert_eq!(buffer.lines().len(), expected);
                assert!(buffer.lines().all(|line| line.as_str().is_empty()));
            }
            buffer.clear();
        }
    }
}

#[test]
fn complete_text_preserves_lines_and_indentation() {
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
            let mut buffer = PreviewBuffer::new();
            match style {
                Some(style) => buffer.push_styled_text(text, style),
                None => buffer.push_text(text),
            }
            assert_eq!(
                buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(buffer.text, expected.concat());
            for (line, &text) in buffer.lines().zip(expected) {
                let expected_spans: Vec<_> = (!text.is_empty())
                    .then(|| style.unwrap_or_default().apply(text))
                    .into_iter()
                    .collect();
                assert_eq!(line.spans().collect::<Vec<_>>(), expected_spans);
            }
        }
    }
}

#[test]
fn complete_text_appends_and_merges_styles_within_lines() {
    let red = ContentStyle::new().red();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_str("prefix ", red);
    buffer.push_styled_text("first\r\n\n  second", red);
    buffer.push_styled_text("", ContentStyle::new().blue());
    buffer.push_text("");
    buffer.push_styled_str(" suffix", red);
    buffer.push_text("!\nlast");
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        ["prefix first", "", "  second suffix!", "last"]
    );
    assert_eq!(
        buffer.line(0).unwrap().spans().collect::<Vec<_>>(),
        [red.apply("prefix first")]
    );
    assert_eq!(buffer.line(1).unwrap().spans().count(), 0);
    assert_eq!(
        buffer.line(2).unwrap().spans().collect::<Vec<_>>(),
        [
            red.apply("  second suffix"),
            ContentStyle::default().apply("!")
        ]
    );
    assert_eq!(buffer.styles.len(), 2);
}

#[test]
fn complete_text_replaces_control_characters() {
    let red = ContentStyle::new().red();
    let mut text: String = (0u8..=31)
        .filter(|&byte| byte != b'\n')
        .map(char::from)
        .collect();
    text.push('\x7f');
    text.extend((0x80u8..=0x9f).map(char::from));
    let expected = "␀␁␂␃␄␅␆␇␈␉␋␌␍␎␏␐␑␒␓␔␕␖␗␘␙␚␛␜␝␞␟␡";
    for style in [None, Some(red)] {
        let mut buffer = PreviewBuffer::new();
        match style {
            Some(style) => buffer.push_styled_text(&text, style),
            None => buffer.push_text(&text),
        }
        assert_eq!(buffer.lines().len(), 1);
        assert_eq!(buffer.line(0).unwrap().as_str(), expected);
        assert_eq!(
            buffer.line(0).unwrap().spans().collect::<Vec<_>>(),
            [style.unwrap_or_default().apply(expected)]
        );
    }
}

#[test]
fn sanitized_text_preserves_line_boundaries_and_styles() {
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_str("prefix ", red);
    buffer.push_styled_text("界\t\u{85}e\u{301}👩‍💻\r\n\r\r\n\x1b[31m\nlast\r", red);
    buffer.push_styled_text("\n\u{80}\u{9f}", blue);
    buffer.push_styled_str("suffix", blue);
    let expected = ["prefix 界␉e\u{301}👩‍💻", "␍", "␛[31m", "last␍", "suffix"];
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        expected
    );
    for (index, line) in buffer.lines().enumerate() {
        let style = if index == 4 { blue } else { red };
        assert_eq!(
            line.spans().collect::<Vec<_>>(),
            [style.apply(expected[index])]
        );
        assert!(!line.as_str().chars().any(char::is_control));
    }
}

#[test]
fn discarded_controls_do_not_create_lines_or_styles() {
    let mut buffer = PreviewBuffer::new();
    let controls: String = (0x80u8..=0x9f).map(char::from).collect();
    buffer.push_styled_text(&controls, ContentStyle::new().red());
    assert!(buffer.text.is_empty());
    assert_eq!(buffer.lines().len(), 1);
    assert_eq!(buffer.line(0).unwrap().as_str(), "");
    assert!(buffer.styles.is_empty());
    buffer.push_text("\u{85}\n\u{9f}");
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        ["", ""]
    );
    assert!(buffer.styles.is_empty());
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
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        ["", "prefix plain", "same style", "", ""]
    );
    assert_eq!(
        buffer.line(1).unwrap().spans().collect::<Vec<_>>(),
        [red.apply("prefix "), ContentStyle::default().apply("plain")]
    );
    assert_eq!(
        buffer.line(2).unwrap().spans().collect::<Vec<_>>(),
        [red.apply("same style")]
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
fn explicit_line_boundaries() {
    let cases: &[&[&str]] = &[
        &[""],
        &["hello"],
        &["hello", ""],
        &["", ""],
        &["", "", ""],
        &["hello", "world", ""],
        &["", "hello", "", "world"],
        &["界", "e\u{301}", "👩‍💻"],
    ];

    for &expected in cases {
        let mut buffer = PreviewBuffer::new();
        for (index, text) in expected.iter().enumerate() {
            if index > 0 {
                buffer.newline();
            }
            buffer.push_str(text);
        }
        assert_eq!(buffer.text, expected.concat());
        assert_eq!(
            buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(buffer.lines().len(), expected.len());
        assert_eq!(
            buffer
                .lines()
                .rev()
                .map(|line| line.as_str())
                .collect::<Vec<_>>(),
            expected.iter().copied().rev().collect::<Vec<_>>()
        );
        for (index, line) in expected.iter().enumerate() {
            let view = buffer.line(index).unwrap();
            assert_eq!(view.as_str(), *line);
            let expected_spans: Vec<_> = (!line.is_empty())
                .then(|| ContentStyle::default().apply(*line))
                .into_iter()
                .collect();
            assert_eq!(view.spans().collect::<Vec<_>>(), expected_spans);
        }
        assert!(buffer.line(expected.len()).is_none());
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
        assert_eq!(
            buffer.line(0).unwrap().spans().collect::<Vec<_>>(),
            [style.apply(text)]
        );

        buffer.newline();
        buffer.newline();
        buffer.push_styled_str(text, style);
        buffer.newline();
        assert_eq!(
            buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
            [text, "", text, ""]
        );
        assert_eq!(buffer.line(1).unwrap().spans().count(), 0);
        assert_eq!(
            buffer.line(2).unwrap().spans().collect::<Vec<_>>(),
            [style.apply(text)]
        );
        assert_eq!(buffer.line(3).unwrap().spans().count(), 0);
        assert_eq!(buffer.styles.len(), 2);
    }

    let mut buffer = PreviewBuffer::new();
    for ch in text.chars() {
        buffer.push_str(ch.encode_utf8(&mut [0; 4]));
    }
    assert_eq!(buffer.text, text);
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        [text]
    );
    assert!(buffer.styles.is_empty());
}

#[test]
fn spans_borrow_text_and_include_default_gaps() {
    let red = ContentStyle::new().red();
    let blue = ContentStyle::new().blue();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_str("", red);
    assert!(buffer.styles.is_empty());
    assert_eq!(buffer.lines().len(), 1);

    buffer.push_str("prefix ");
    buffer.push_styled_str("é", red);
    buffer.newline();
    buffer.push_styled_str("界", red);
    buffer.push_str(" ");
    buffer.push_styled_str("e", red);
    buffer.push_styled_str("\u{301}", blue);
    buffer.push_styled_str("!", ContentStyle::default());
    buffer.push_styled_str("?", blue);
    buffer.push_str(" tail");

    let first = buffer.line(0).unwrap();
    assert_eq!(
        first.spans().collect::<Vec<_>>(),
        [ContentStyle::default().apply("prefix "), red.apply("é")]
    );
    let second = buffer.line(1).unwrap();
    assert_eq!(
        second.spans().collect::<Vec<_>>(),
        [
            red.apply("界"),
            ContentStyle::default().apply(" "),
            red.apply("e"),
            blue.apply("\u{301}"),
            ContentStyle::default().apply("!"),
            blue.apply("?"),
            ContentStyle::default().apply(" tail"),
        ]
    );
    for line in [first, second] {
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
                    assert_eq!(
                        buffer.line(0).unwrap().spans().collect::<Vec<_>>(),
                        [red.apply("previous")]
                    );
                    assert_eq!(
                        buffer.line(1).unwrap().spans().collect::<Vec<_>>(),
                        [
                            ContentStyle::default().apply("prefix "),
                            red.apply("current")
                        ]
                    );
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
        assert_eq!(
            view.spans().collect::<Vec<_>>(),
            [
                ContentStyle::default().apply(view.as_str().split_at(6).0),
                red.apply("red"),
                blue.apply(" blue"),
            ]
        );
    }

    let mut lines = buffer.lines();
    assert_eq!(lines.nth(1000).unwrap().as_str(), "1000: red blue");
    assert_eq!(lines.len(), 100);
    assert_eq!(lines.next_back().unwrap().as_str(), "");
    assert_eq!(lines.nth_back(49).unwrap().as_str(), "1050: red blue");
    assert_eq!(lines.len(), 49);
    assert!(lines.nth(usize::MAX).is_none());
    assert_eq!(lines.len(), 0);
}

#[test]
fn unstyled_segments_use_default_style() {
    let mut buffer = PreviewBuffer::new();
    let style = ContentStyle::new().bold();
    buffer.push_styled_str("Size: ", style);
    buffer.push_str("42 bytes");
    buffer.newline();
    buffer.push_str("界");
    buffer.newline();
    buffer.push_str("done");

    assert_eq!(buffer.text, "Size: 42 bytes界done");
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        ["Size: 42 bytes", "界", "done"]
    );
    assert_eq!(
        buffer.line(0).unwrap().spans().collect::<Vec<_>>(),
        [
            style.apply("Size: "),
            ContentStyle::default().apply("42 bytes"),
        ]
    );
    assert_eq!(buffer.styles.len(), 1);
}

#[test]
fn clear_reuses_all_allocations() {
    let mut buffer = PreviewBuffer::new();
    let style = ContentStyle::new().green();
    buffer.push_styled_str("first", style);
    buffer.newline();
    buffer.push_styled_str("second", style);
    buffer.newline();
    buffer.push_styled_str("third", style);
    buffer.is_err = true;
    buffer.set_line_numbers(true);
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
    assert_eq!(buffer.text, "");
    assert_eq!(buffer.lines().len(), 1);
    assert_eq!(buffer.line(0).unwrap().as_str(), "");
    assert_eq!(buffer.line(0).unwrap().spans().count(), 0);
    assert!(buffer.styles.is_empty());
    assert!(!buffer.is_err);
    assert!(!buffer.line_numbers());
    assert_eq!(
        capacities,
        (
            buffer.text.capacity(),
            buffer.lines.capacity(),
            buffer.styles.capacity(),
        )
    );

    buffer.push_styled_str("new", style);
    buffer.newline();
    buffer.push_styled_str("text", style);
    assert_eq!(
        buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
        ["new", "text"]
    );
    for line in buffer.lines() {
        assert_eq!(
            line.spans().collect::<Vec<_>>(),
            [style.apply(line.as_str())]
        );
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
