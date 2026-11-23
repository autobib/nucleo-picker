use super::*;
use crate::{
    preview::{Buffer, Content},
    util::unicode::{Processor, UnicodeProcessor},
};

struct Run {
    text: String,
    spans: Vec<(String, Style)>,
}

struct RecordingSurface {
    width: u16,
    height: u16,
    row: u16,
    column: u16,
    runs: Vec<Run>,
}

impl RecordingSurface {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            row: height,
            column: 0,
            runs: Vec::new(),
        }
    }
}

impl Surface for RecordingSurface {
    type Error = !;

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn begin_row(&mut self, row: u16, _clear: bool) -> Result<(), Self::Error> {
        assert!(self.width != 0 && row < self.height);
        assert!(self.row == self.height || row > self.row);
        self.row = row;
        self.column = 0;
        Ok(())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        spans: impl Iterator<Item = StyledStr<'a>>,
    ) -> Result<(), Self::Error> {
        if text.is_empty() {
            return Ok(());
        }
        assert!(self.width != 0 && self.row < self.height);
        let width = UnicodeProcessor::width(text);
        let end = usize::from(self.column) + width;
        assert!(end <= usize::from(self.width), "text overflows: {text:?}");
        self.column = end as u16;
        let spans: Vec<_> = spans
            .map(|span| (span.text.to_owned(), span.style))
            .collect();
        assert_eq!(
            spans
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>(),
            text
        );
        self.runs.push(Run {
            text: text.to_owned(),
            spans,
        });
        Ok(())
    }

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> Result<(), Self::Error> {
        let text = text.to_string();
        assert!(text.bytes().all(|byte| (b' '..=b'~').contains(&byte)));
        self.text(&text, style)
    }

    fn spaces(&mut self, width: u16, style: Style) -> Result<(), Self::Error> {
        self.text(&" ".repeat(usize::from(width)), style)
    }
}

#[test]
fn preview_passes_whole_graphemes_and_preserves_text_styles() {
    let spans = [
        ("e", Style::new().fg(Color::Red)),
        ("\u{301}👩", Style::new().fg(Color::Blue)),
        ("🏽‍", Style::new().fg(Color::Green)),
        ("💻", Style::new().fg(Color::Yellow)),
    ];
    let mut buffer = Buffer::new();
    for &(text, style) in &spans {
        buffer.push_styled_str(text, style);
    }
    let pane = buffer.into_pane();
    let mut target = RecordingSurface::new(8, 1);
    crate::preview::draw(&mut target, Content::Ready(&pane), None, '…').unwrap();
    let graphemes: Vec<_> = target
        .runs
        .iter()
        .flat_map(|run| UnicodeProcessor::grapheme_indices(&run.text).map(|(_, text)| text))
        .collect();
    assert_eq!(graphemes, ["e\u{301}", "👩🏽‍💻", " ", " ", " ", " ", " "]);
    let actual: Vec<_> = target
        .runs
        .iter()
        .flat_map(|run| &run.spans)
        .flat_map(|(text, style)| text.chars().map(move |ch| (ch, *style)))
        .collect();
    let expected: Vec<_> = spans
        .into_iter()
        .chain(std::iter::once(("     ", Style::new())))
        .flat_map(|(text, style)| text.chars().map(move |ch| (ch, style)))
        .collect();
    assert_eq!(actual, expected);
}
