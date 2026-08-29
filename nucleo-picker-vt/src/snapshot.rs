use libghostty_vt::render::{CellIteration, CellIterator, Colors, CursorVisualStyle, RowIterator};
use libghostty_vt::screen::{CellWide, Screen};
use libghostty_vt::style::{RgbColor, StyleColor, Underline};
use libghostty_vt::{Error, RenderState, Terminal};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub version: u32,
    pub size: Size,
    pub screen: ScreenName,
    pub cursor: Cursor,
    pub colors: DefaultColors,
    pub text: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_flags: Vec<RowFlags>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<StyleSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenName {
    Primary,
    Alternate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub position: Option<Position>,
    pub visible: bool,
    pub shape: CursorShape,
    pub blinking: bool,
    pub pending_wrap: bool,
    pub at_wide_tail: bool,
    pub password_input: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
    BlockHollow,
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultColors {
    pub foreground: HexColor,
    pub background: HexColor,
    pub cursor: Option<HexColor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HexColor(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowFlags {
    pub row: u16,
    pub wrapped: bool,
    pub continuation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleSpan {
    pub row: u16,
    pub start: u16,
    pub end: u16,
    #[serde(flatten)]
    pub style: CellStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CellStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline_color: Option<HexColor>,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub faint: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub blink: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub inverse: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub invisible: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strikethrough: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub overline: bool,
    #[serde(skip_serializing_if = "UnderlineName::is_none")]
    pub underline: UnderlineName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnderlineName {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
    Unknown(u32),
}

impl UnderlineName {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub(crate) struct Snapshotter<'alloc> {
    render: RenderState<'alloc>,
    rows: RowIterator<'alloc>,
    cells: CellIterator<'alloc>,
}

impl Snapshotter<'static> {
    pub(crate) fn new() -> Result<Self, Error> {
        Ok(Self {
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
        })
    }
}

impl<'alloc> Snapshotter<'alloc> {
    pub(crate) fn capture<'cb>(
        &mut self,
        terminal: &Terminal<'alloc, 'cb>,
    ) -> Result<PaneSnapshot, Error>
    where
        'alloc: 'cb,
    {
        let snapshot = self.render.update(terminal)?;
        let cols = snapshot.cols()?;
        let rows = snapshot.rows()?;
        let colors = snapshot.colors()?;
        let cursor_viewport = snapshot.cursor_viewport()?;
        let cursor = Cursor {
            position: cursor_viewport.map(|position| Position {
                x: position.x,
                y: position.y,
            }),
            visible: snapshot.cursor_visible()?,
            shape: cursor_shape(snapshot.cursor_visual_style()?),
            blinking: snapshot.cursor_blinking()?,
            pending_wrap: terminal.is_cursor_pending_wrap()?,
            at_wide_tail: cursor_viewport.is_some_and(|position| position.at_wide_tail),
            password_input: snapshot.cursor_password_input()?,
        };

        let mut text = Vec::with_capacity(rows as usize);
        let mut row_flags = Vec::new();
        let mut styles: Vec<StyleSpan> = Vec::new();
        let mut row_iter = self.rows.update(&snapshot)?;
        let mut y = 0_u16;

        while let Some(row) = row_iter.next() {
            let raw_row = row.raw_row()?;
            let wrapped = raw_row.is_wrapped()?;
            let continuation = raw_row.is_wrap_continuation()?;
            if wrapped || continuation {
                row_flags.push(RowFlags {
                    row: y,
                    wrapped,
                    continuation,
                });
            }

            let mut row_text = String::new();
            let mut cell_iter = self.cells.update(row)?;
            let mut x = 0_u16;
            while let Some(cell) = cell_iter.next() {
                push_cell_text(cell, &mut row_text)?;
                let style = owned_style(cell, &colors)?;
                if style != CellStyle::default() {
                    match styles.last_mut() {
                        Some(last) if last.row == y && last.end == x && last.style == style => {
                            last.end += 1;
                        }
                        _ => styles.push(StyleSpan {
                            row: y,
                            start: x,
                            end: x + 1,
                            style,
                        }),
                    }
                }
                x += 1;
            }

            debug_assert_eq!(x, cols);
            text.push(row_text);
            y += 1;
        }
        debug_assert_eq!(y, rows);

        Ok(PaneSnapshot {
            version: 1,
            size: Size { cols, rows },
            screen: match terminal.active_screen()? {
                Screen::Primary => ScreenName::Primary,
                Screen::Alternate => ScreenName::Alternate,
            },
            cursor,
            colors: DefaultColors {
                foreground: colors.foreground.into(),
                background: colors.background.into(),
                cursor: colors.cursor.map(Into::into),
            },
            text,
            row_flags,
            styles,
        })
    }
}

fn push_cell_text(cell: &CellIteration<'_, '_>, output: &mut String) -> Result<(), Error> {
    let mut grapheme = String::new();
    cell.graphemes_utf8(&mut grapheme)?;
    if grapheme.is_empty() && cell.raw_cell()?.wide()? != CellWide::SpacerTail {
        output.push(' ');
    } else {
        output.push_str(&grapheme);
    }
    Ok(())
}

fn owned_style(cell: &CellIteration<'_, '_>, colors: &Colors) -> Result<CellStyle, Error> {
    let style = cell.style()?;
    Ok(CellStyle {
        foreground: cell.fg_color()?.map(Into::into),
        background: cell.bg_color()?.map(Into::into),
        underline_color: resolve_color(style.underline_color, colors).map(Into::into),
        bold: style.bold,
        italic: style.italic,
        faint: style.faint,
        blink: style.blink,
        inverse: style.inverse,
        invisible: style.invisible,
        strikethrough: style.strikethrough,
        overline: style.overline,
        underline: underline_name(style.underline),
    })
}

fn resolve_color(color: StyleColor, colors: &Colors) -> Option<RgbColor> {
    match color {
        StyleColor::None => None,
        StyleColor::Rgb(rgb) => Some(rgb),
        StyleColor::Palette(index) => Some(colors.palette[index.0 as usize]),
    }
}

fn cursor_shape(value: CursorVisualStyle) -> CursorShape {
    match value {
        CursorVisualStyle::Bar => CursorShape::Bar,
        CursorVisualStyle::Block => CursorShape::Block,
        CursorVisualStyle::Underline => CursorShape::Underline,
        CursorVisualStyle::BlockHollow => CursorShape::BlockHollow,
        _ => CursorShape::Unknown(value.into()),
    }
}

fn underline_name(value: Underline) -> UnderlineName {
    match value {
        Underline::None => UnderlineName::None,
        Underline::Single => UnderlineName::Single,
        Underline::Double => UnderlineName::Double,
        Underline::Curly => UnderlineName::Curly,
        Underline::Dotted => UnderlineName::Dotted,
        Underline::Dashed => UnderlineName::Dashed,
        _ => UnderlineName::Unknown(value.into()),
    }
}

impl From<RgbColor> for HexColor {
    fn from(value: RgbColor) -> Self {
        Self(format!("#{:02x}{:02x}{:02x}", value.r, value.g, value.b))
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use crossterm::{
        queue,
        style::{PrintStyledContent, Stylize, force_color_output},
    };
    use libghostty_vt::{Terminal, TerminalOptions};

    use super::*;

    #[test]
    fn captures_owned_viewport_text_cursor_styles_and_wraps() -> Result<(), Box<dyn StdError>> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 6,
            rows: 2,
            max_scrollback: 0,
        })?;
        terminal.vt_write(b"a\x1b[31;1mB\x1b[0m\xe7\x95\x8cc");

        let mut snapshotter = Snapshotter::new()?;
        let snapshot = snapshotter.capture(&terminal)?;

        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.size, Size { cols: 6, rows: 2 });
        assert_eq!(snapshot.screen, ScreenName::Primary);
        assert_eq!(snapshot.text, ["aB界c ", "      "]);
        assert_eq!(snapshot.cursor.position, Some(Position { x: 5, y: 0 }));
        assert_eq!(snapshot.styles.len(), 1);
        assert_eq!(
            (
                snapshot.styles[0].row,
                snapshot.styles[0].start,
                snapshot.styles[0].end
            ),
            (0, 1, 2)
        );
        assert!(snapshot.styles[0].style.foreground.is_some());
        assert!(snapshot.styles[0].style.bold);
        assert!(snapshot.row_flags.is_empty());

        terminal.vt_write(b"de");
        let wrapped = snapshotter.capture(&terminal)?;
        assert_eq!(wrapped.text, ["aB界cd", "e     "]);
        assert_eq!(
            wrapped.row_flags,
            [
                RowFlags {
                    row: 0,
                    wrapped: true,
                    continuation: false,
                },
                RowFlags {
                    row: 1,
                    wrapped: false,
                    continuation: true,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn captures_forced_crossterm_color() -> Result<(), Box<dyn StdError>> {
        force_color_output(true);
        let mut output = Vec::new();
        queue!(output, PrintStyledContent("x".cyan()))?;

        let mut terminal = Terminal::new(TerminalOptions {
            cols: 1,
            rows: 1,
            max_scrollback: 0,
        })?;
        terminal.vt_write(&output);

        let snapshot = Snapshotter::new()?.capture(&terminal)?;

        assert_eq!(snapshot.styles.len(), 1);
        assert!(snapshot.styles[0].style.foreground.is_some());
        Ok(())
    }

    #[test]
    fn preserves_combining_graphemes() -> Result<(), Box<dyn StdError>> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: 4,
            rows: 1,
            max_scrollback: 0,
        })?;
        terminal.vt_write("e\u{301}".as_bytes());

        let snapshot = Snapshotter::new()?.capture(&terminal)?;

        assert_eq!(snapshot.text, ["e\u{301}   "]);
        Ok(())
    }
}
