use std::convert::Infallible;

use crossterm::style::Stylize;

use super::*;
use crate::{
    PickerChars,
    component::Component,
    preview::{Buffer, Content},
    prompt::{Prompt, PromptState},
    status_line::StatusLine,
    util::unicode::{Processor, UnicodeProcessor},
};

#[derive(Debug, PartialEq, Eq)]
struct Run {
    row: u16,
    text: String,
    spans: Vec<(String, ContentStyle)>,
}

struct RecordingSurface {
    width: u16,
    height: u16,
    row: u16,
    column: u16,
    runs: Vec<Run>,
    cleared: Vec<u16>,
}

impl RecordingSurface {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            row: 0,
            column: 0,
            runs: Vec::new(),
            cleared: Vec::new(),
        }
    }
}

impl Surface for RecordingSurface {
    type Error = Infallible;

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
            row: self.row,
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
        self.cleared.push(self.row);
        self.column = 0;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        if self.is_empty() {
            return Ok(());
        }
        self.cleared.extend(0..self.height);
        self.row = 0;
        self.column = 0;
        Ok(())
    }
}

#[test]
fn emptiness_depends_on_either_axis_without_collapsing_dimensions() {
    for (width, height) in [(0, 0), (0, 3), (6, 0), (6, 3)] {
        let mut target = RecordingSurface::new(width, height);
        assert_eq!(target.width(), width);
        assert_eq!(target.height(), height);
        assert_eq!(target.is_empty(), width == 0 || height == 0);
        target.clear().unwrap();
        assert_eq!(
            target.cleared.len(),
            if target.is_empty() {
                0
            } else {
                usize::from(height)
            }
        );
    }
}

#[test]
fn prompt_and_status_draw_to_an_infallible_target() {
    let chars = PickerChars::new();
    let mut state = PromptState::new(Default::default());
    let mut prompt = Prompt::new(&mut state, &chars);
    let mut target = RecordingSurface::new(20, 1);
    prompt.draw(&mut target).unwrap();
    assert_eq!(target.cleared, [0]);
    assert_eq!(target.runs[0].text, ">");
    assert!(
        target
            .runs
            .iter()
            .flat_map(|run| &run.spans)
            .all(|(_, style)| *style == ContentStyle::new())
    );

    let mut target = RecordingSurface::new(20, 1);
    StatusLine::new(&chars).draw(&mut target).unwrap();
    assert!(target.runs.iter().any(|run| run.text == " 0/0"));
    assert!(
        target
            .runs
            .iter()
            .flat_map(|run| &run.spans)
            .all(|(_, style)| style.attributes.has(crossterm::style::Attribute::Italic))
    );
}

#[test]
fn preview_passes_whole_graphemes_with_original_style_boundaries() {
    let mut buffer = Buffer::new();
    buffer.push_styled_str("e", ContentStyle::new().red());
    buffer.push_styled_str("\u{301}👩", ContentStyle::new().blue());
    buffer.push_styled_str("🏽‍", ContentStyle::new().green());
    buffer.push_styled_str("💻", ContentStyle::new().yellow());
    let pane = buffer.into_pane();
    let mut target = RecordingSurface::new(8, 1);
    crate::preview::draw(&mut target, Content::Ready(&pane), None, '…').unwrap();
    let run = target
        .runs
        .iter()
        .find(|run| run.text == "e\u{301}👩🏽‍💻")
        .unwrap();
    assert_eq!(
        run.spans,
        [
            ("e".to_owned(), ContentStyle::new().red()),
            ("\u{301}👩".to_owned(), ContentStyle::new().blue()),
            ("🏽‍".to_owned(), ContentStyle::new().green()),
            ("💻".to_owned(), ContentStyle::new().yellow()),
        ]
    );
    assert_eq!(target.runs.last().unwrap().spans[0].1, ContentStyle::new());
}
