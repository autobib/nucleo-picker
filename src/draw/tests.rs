use crossterm::style::Stylize;

use super::*;
use crate::{
    preview::{Buffer, Content},
    util::unicode::{Processor, UnicodeProcessor},
};

struct Run {
    text: String,
    spans: Vec<(String, ContentStyle)>,
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
            row: 0,
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

    fn move_to(&mut self, column: u16, row: u16) -> Result<(), Self::Error> {
        assert!(column < self.width && row < self.height);
        self.row = row;
        self.column = column;
        Ok(())
    }

    fn move_to_column(&mut self, column: u16) -> Result<(), Self::Error> {
        self.move_to(column, self.row)
    }

    fn next_line(&mut self) -> Result<(), Self::Error> {
        assert!(self.width != 0 && self.row < self.height);
        self.row += 1;
        self.column = 0;
        Ok(())
    }

    fn styled_text<'a>(
        &mut self,
        text: &'a str,
        spans: impl Iterator<Item = StyledContent<&'a str>>,
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
            .map(|span| ((*span.content()).to_owned(), *span.style()))
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

    fn ascii(&mut self, text: fmt::Arguments<'_>, style: ContentStyle) -> Result<(), Self::Error> {
        let text = text.to_string();
        assert!(text.bytes().all(|byte| (b' '..=b'~').contains(&byte)));
        self.text(&text, style)
    }

    fn spaces(&mut self, width: u16, style: ContentStyle) -> Result<(), Self::Error> {
        self.text(&" ".repeat(usize::from(width)), style)
    }

    fn clear_line(&mut self) -> Result<(), Self::Error> {
        assert!(self.width != 0 && self.row < self.height);
        self.column = 0;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        if self.is_empty() {
            return Ok(());
        }
        self.row = 0;
        self.column = 0;
        Ok(())
    }
}

#[test]
fn preview_passes_whole_graphemes_and_preserves_text_styles() {
    let spans = [
        ("e", ContentStyle::new().red()),
        ("\u{301}👩", ContentStyle::new().blue()),
        ("🏽‍", ContentStyle::new().green()),
        ("💻", ContentStyle::new().yellow()),
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
        .chain(std::iter::once(("     ", ContentStyle::new())))
        .flat_map(|(text, style)| text.chars().map(move |ch| (ch, style)))
        .collect();
    assert_eq!(actual, expected);
}
