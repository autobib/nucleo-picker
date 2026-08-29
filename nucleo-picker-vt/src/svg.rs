use std::io::Write;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use xml::writer::{EmitterConfig, EventWriter, Result, XmlEvent};

use crate::{CellStyle, CursorShape, HexColor, PaneSnapshot, UnderlineName};

const CELL_WIDTH: u32 = 10;
const CELL_HEIGHT: u32 = 20;
const BASELINE: u32 = 15;

impl PaneSnapshot {
    pub fn write_svg<W: Write>(&self, output: W) -> Result<()> {
        let width = (u32::from(self.size.cols) * CELL_WIDTH).to_string();
        let height = (u32::from(self.size.rows) * CELL_HEIGHT).to_string();
        let view_box = format!("0 0 {width} {height}");
        let mut writer = EmitterConfig::new()
            .write_document_declaration(false)
            .pad_self_closing(false)
            .create_writer(output);

        writer.write(
            XmlEvent::start_element("svg")
                .default_ns("http://www.w3.org/2000/svg")
                .attr("width", &width)
                .attr("height", &height)
                .attr("viewBox", &view_box)
                .attr("role", "img")
                .attr("xml:space", "preserve")
                .attr("font-family", "monospace")
                .attr("font-size", "14"),
        )?;
        empty_element(
            &mut writer,
            "rect",
            &[
                ("width", "100%"),
                ("height", "100%"),
                ("fill", &self.colors.background.0),
            ],
        )?;

        self.write_style_backgrounds(&mut writer)?;
        self.write_cursor(&mut writer)?;
        self.write_text(&mut writer)?;
        writer.write(XmlEvent::end_element())
    }

    fn write_style_backgrounds<W: Write>(&self, writer: &mut EventWriter<W>) -> Result<()> {
        for span in &self.styles {
            if span.row >= self.size.rows {
                continue;
            }
            let start = span.start.min(self.size.cols);
            let end = span.end.min(self.size.cols);
            if start >= end {
                continue;
            }
            let (_, background) = self.style_colors(&span.style);
            if background.0 == self.colors.background.0 {
                continue;
            }

            let x = (u32::from(start) * CELL_WIDTH).to_string();
            let y = (u32::from(span.row) * CELL_HEIGHT).to_string();
            let width = (u32::from(end - start) * CELL_WIDTH).to_string();
            let height = CELL_HEIGHT.to_string();
            empty_element(
                writer,
                "rect",
                &[
                    ("x", &x),
                    ("y", &y),
                    ("width", &width),
                    ("height", &height),
                    ("fill", &background.0),
                ],
            )?;
        }
        Ok(())
    }

    fn write_cursor<W: Write>(&self, writer: &mut EventWriter<W>) -> Result<()> {
        let Some(position) = self.cursor.position.filter(|_| self.cursor.visible) else {
            return Ok(());
        };
        if position.x >= self.size.cols || position.y >= self.size.rows {
            return Ok(());
        }

        let cursor_x = if self.cursor.at_wide_tail {
            position.x.saturating_sub(1)
        } else {
            position.x
        };
        let cell_count = if self.cursor.at_wide_tail { 2 } else { 1 };
        let x = (u32::from(cursor_x) * CELL_WIDTH).to_string();
        let y = (u32::from(position.y) * CELL_HEIGHT).to_string();
        let width = (CELL_WIDTH * cell_count).to_string();
        let height = CELL_HEIGHT.to_string();
        let color = self
            .colors
            .cursor
            .as_ref()
            .unwrap_or(&self.colors.foreground);

        match self.cursor.shape {
            CursorShape::Bar => empty_element(
                writer,
                "rect",
                &[
                    ("x", &x),
                    ("y", &y),
                    ("width", "2"),
                    ("height", &height),
                    ("fill", &color.0),
                ],
            ),
            CursorShape::Underline => {
                let y = (u32::from(position.y) * CELL_HEIGHT + CELL_HEIGHT - 2).to_string();
                empty_element(
                    writer,
                    "rect",
                    &[
                        ("x", &x),
                        ("y", &y),
                        ("width", &width),
                        ("height", "2"),
                        ("fill", &color.0),
                    ],
                )
            }
            CursorShape::BlockHollow => empty_element(
                writer,
                "rect",
                &[
                    ("x", &x),
                    ("y", &y),
                    ("width", &width),
                    ("height", &height),
                    ("fill", "none"),
                    ("stroke", &color.0),
                ],
            ),
            CursorShape::Block | CursorShape::Unknown(_) => empty_element(
                writer,
                "rect",
                &[
                    ("x", &x),
                    ("y", &y),
                    ("width", &width),
                    ("height", &height),
                    ("fill", &color.0),
                ],
            ),
        }
    }

    fn write_text<W: Write>(&self, writer: &mut EventWriter<W>) -> Result<()> {
        for (row, text) in self.text.iter().take(self.size.rows.into()).enumerate() {
            let row = row as u16;
            let mut column = 0_u16;
            for grapheme in text.graphemes(true) {
                if column >= self.size.cols {
                    break;
                }
                let grapheme_width = UnicodeWidthStr::width(grapheme).max(1);
                let style = self
                    .styles
                    .iter()
                    .find(|span| span.row == row && span.start <= column && column < span.end)
                    .map(|span| &span.style);
                if !style.is_some_and(|style| style.invisible) {
                    self.write_grapheme(writer, row, column, grapheme, style)?;
                }
                column = column.saturating_add(u16::try_from(grapheme_width).unwrap_or(u16::MAX));
            }
        }
        Ok(())
    }

    fn write_grapheme<W: Write>(
        &self,
        writer: &mut EventWriter<W>,
        row: u16,
        column: u16,
        grapheme: &str,
        style: Option<&CellStyle>,
    ) -> Result<()> {
        let x = (u32::from(column) * CELL_WIDTH).to_string();
        let y = (u32::from(row) * CELL_HEIGHT + BASELINE).to_string();
        let (foreground, _) = style.map_or_else(
            || (&self.colors.foreground, &self.colors.background),
            |style| self.style_colors(style),
        );
        let decorations = style.map_or("", decoration_names);
        let mut element = XmlEvent::start_element("text")
            .attr("x", &x)
            .attr("y", &y)
            .attr("fill", &foreground.0);

        if let Some(style) = style {
            if style.bold {
                element = element.attr("font-weight", "bold");
            }
            if style.italic {
                element = element.attr("font-style", "italic");
            }
            if style.faint {
                element = element.attr("fill-opacity", "0.5");
            }
            if !decorations.is_empty() {
                element = element.attr("text-decoration", decorations);
            }
            if !matches!(style.underline, UnderlineName::None) {
                element = element.attr("text-decoration-style", underline_style(style.underline));
            }
            if let Some(color) = &style.underline_color {
                element = element.attr("text-decoration-color", &color.0);
            }
        }

        writer.write(element)?;
        writer.write(XmlEvent::characters(grapheme))?;
        writer.write(XmlEvent::end_element())
    }

    fn style_colors<'a>(&'a self, style: &'a CellStyle) -> (&'a HexColor, &'a HexColor) {
        let foreground = style.foreground.as_ref().unwrap_or(&self.colors.foreground);
        let background = style.background.as_ref().unwrap_or(&self.colors.background);
        if style.inverse {
            (background, foreground)
        } else {
            (foreground, background)
        }
    }
}

fn decoration_names(style: &CellStyle) -> &'static str {
    match (
        !matches!(style.underline, UnderlineName::None),
        style.strikethrough,
        style.overline,
    ) {
        (false, false, false) => "",
        (false, false, true) => "overline",
        (false, true, false) => "line-through",
        (false, true, true) => "line-through overline",
        (true, false, false) => "underline",
        (true, false, true) => "underline overline",
        (true, true, false) => "underline line-through",
        (true, true, true) => "underline line-through overline",
    }
}

fn underline_style(underline: UnderlineName) -> &'static str {
    match underline {
        UnderlineName::None | UnderlineName::Single | UnderlineName::Unknown(_) => "solid",
        UnderlineName::Double => "double",
        UnderlineName::Curly => "wavy",
        UnderlineName::Dotted => "dotted",
        UnderlineName::Dashed => "dashed",
    }
}

fn empty_element<'a, W: Write>(
    writer: &mut EventWriter<W>,
    name: &'a str,
    attributes: &[(&'a str, &'a str)],
) -> Result<()> {
    let mut element = XmlEvent::start_element(name);
    for &(name, value) in attributes {
        element = element.attr(name, value);
    }
    writer.write(element)?;
    writer.write(XmlEvent::end_element())
}
