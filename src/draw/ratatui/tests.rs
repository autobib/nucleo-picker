use crossterm::style::{Color as CrosstermColor, Stylize};
use ratatui_core::style::Color;

use super::*;
use crate::{
    PickerChars,
    component::Component,
    preview::Content,
    prompt::{Prompt, PromptEvent, PromptState},
    render::StrRenderer,
    session::DriverOptions,
    status_line::StatusLine,
};

fn dirty_cell() -> Cell {
    let mut cell = Cell::new(".");
    cell.set_style(
        Style::new()
            .fg(Color::Red)
            .bg(Color::Blue)
            .underline_color(Color::Green)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
    .set_diff_option(CellDiffOption::Skip);
    cell
}

fn assert_outside_unchanged(buffer: &Buffer, area: Rect, expected: &Cell) {
    for y in buffer.area.y..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            if !area.contains((x, y).into()) {
                assert_eq!(&buffer[(x, y)], expected, "outside at ({x}, {y})");
            }
        }
    }
}

#[test]
fn construction_and_movement_use_rectangle_local_coordinates() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 10, 5), dirty_cell());
    let area = Rect::new(7, 8, 6, 3);
    {
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        assert_eq!(rect.width(), 6);
        assert_eq!(rect.height(), 3);
        assert!(!rect.is_empty());
        rect.move_to(3, 1).unwrap();
        rect.text("a", ContentStyle::new()).unwrap();
        rect.move_to_column(1).unwrap();
        rect.text("b", ContentStyle::new()).unwrap();
        rect.next_line().unwrap();
        rect.text("123456", ContentStyle::new()).unwrap();
        rect.next_line().unwrap();
        rect.move_to(0, 0).unwrap();
        rect.text("c", ContentStyle::new()).unwrap();
    }
    assert_eq!(buffer[(10, 9)], Cell::new("a"));
    assert_eq!(buffer[(8, 9)], Cell::new("b"));
    assert_eq!(buffer[(7, 8)], Cell::new("c"));
    for (x, ch) in (7..13).zip("123456".chars()) {
        assert_eq!(buffer[(x, 10)], Cell::from(ch));
    }
    assert_eq!(buffer[(12, 8)], dirty_cell());
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}

#[test]
#[should_panic(expected = "contained in the buffer")]
fn construction_rejects_a_partially_contained_rectangle() {
    let mut buffer = Buffer::empty(Rect::new(5, 7, 10, 5));
    RatatuiSurface::new(&mut buffer, Rect::new(6, 8, 10, 2));
}

#[test]
fn empty_rectangles_preserve_dimensions_and_leave_buffers_unchanged() {
    for bounds in [
        Rect::new(5, 7, 10, 5),
        Rect::new(5, 7, 0, 5),
        Rect::new(5, 7, 10, 0),
        Rect::new(5, 7, 0, 0),
    ] {
        let mut buffer = Buffer::filled(bounds, dirty_cell());
        let before = buffer.clone();
        for area in [
            Rect::new(bounds.x, bounds.y, 0, bounds.height),
            Rect::new(bounds.x, bounds.y, bounds.width, 0),
            Rect::new(bounds.right(), bounds.y, 0, bounds.height),
            Rect::new(bounds.x, bounds.bottom(), bounds.width, 0),
            Rect::new(bounds.right(), bounds.bottom(), 0, 0),
        ] {
            let mut rect = RatatuiSurface::new(&mut buffer, area);
            assert_eq!(rect.width(), area.width);
            assert_eq!(rect.height(), area.height);
            assert!(rect.is_empty());
            rect.clear().unwrap();
            rect.clear().unwrap();
            assert_eq!(buffer, before);
        }
    }
}

#[test]
fn empty_rectangles_still_require_containment() {
    for area in [
        Rect::new(4, 7, 0, 5),
        Rect::new(5, 6, 10, 0),
        Rect::new(16, 7, 0, 5),
        Rect::new(5, 13, 10, 0),
        Rect::new(15, 8, 0, 5),
        Rect::new(6, 12, 10, 0),
    ] {
        let mut buffer = Buffer::empty(Rect::new(5, 7, 10, 5));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                RatatuiSurface::new(&mut buffer, area);
            }))
            .is_err(),
            "{area:?}"
        );
    }
}

#[test]
fn empty_rectangles_reject_cursor_operations_and_nonempty_writes() {
    let operations: [fn(&mut RatatuiSurface<'_>); 9] = [
        |rect| rect.move_to(0, 0).unwrap(),
        |rect| rect.move_to_column(0).unwrap(),
        |rect| rect.next_line().unwrap(),
        |rect| rect.clear_line().unwrap(),
        |rect| rect.text("x", ContentStyle::new()).unwrap(),
        |rect| rect.text("\u{200b}", ContentStyle::new()).unwrap(),
        |rect| {
            rect.styled_text("x", std::iter::once(ContentStyle::new().apply("x")))
                .unwrap();
        },
        |rect| rect.ascii(format_args!("x"), ContentStyle::new()).unwrap(),
        |rect| rect.spaces(1, ContentStyle::new()).unwrap(),
    ];
    for (width, height) in [(0, 0), (0, 3), (6, 0)] {
        let area = Rect::new(0, 0, width, height);
        for operation in operations {
            let mut buffer = Buffer::empty(area);
            let mut rect = RatatuiSurface::new(&mut buffer, area);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    operation(&mut rect);
                }))
                .is_err()
            );
        }
    }
}

#[test]
#[should_panic(expected = "drawing cursor is invalid")]
fn advancing_past_the_last_row_invalidates_the_cursor() {
    let area = Rect::new(0, 0, 1, 1);
    let mut buffer = Buffer::empty(area);
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.next_line().unwrap();
    rect.text("x", ContentStyle::new()).unwrap();
}

#[test]
fn clears_reset_only_the_assigned_cells_and_spaces_paint_their_style() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 10, 5), dirty_cell());
    let area = Rect::new(7, 8, 6, 3);
    {
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        rect.move_to(4, 1).unwrap();
        rect.clear_line().unwrap();
        rect.text("x", ContentStyle::new()).unwrap();
        rect.next_line().unwrap();
        rect.spaces(3, ContentStyle::new().on_dark_red()).unwrap();
    }
    assert_eq!(buffer[(7, 9)], Cell::new("x"));
    for x in 8..13 {
        assert_eq!(buffer[(x, 9)], Cell::EMPTY);
    }
    for x in 7..10 {
        let mut expected = Cell::EMPTY;
        expected.set_bg(Color::Red);
        assert_eq!(buffer[(x, 10)], expected);
    }
    assert_eq!(buffer[(10, 10)], dirty_cell());
    assert_outside_unchanged(&buffer, area, &dirty_cell());

    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.clear().unwrap();
    rect.text("y", ContentStyle::new()).unwrap();
    for y in 8..11 {
        for x in 7..13 {
            let expected = if (x, y) == (7, 8) {
                Cell::new("y")
            } else {
                Cell::EMPTY
            };
            assert_eq!(buffer[(x, y)], expected);
        }
    }
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}

#[test]
fn styled_text_segments_the_whole_source_and_uses_each_graphemes_first_style() {
    let area = Rect::new(3, 4, 4, 1);
    let mut buffer = Buffer::filled(area, dirty_cell());
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.styled_text(
        "e\u{301}👩🏽‍💻x",
        [
            "".white(),
            "e".red().bold().underline(CrosstermColor::Cyan),
            "\u{301}👩".blue().italic(),
            "🏽‍".green(),
            "".red(),
            "💻".yellow(),
            ContentStyle::new().apply("x"),
        ]
        .into_iter(),
    )
    .unwrap();
    let mut accent = Cell::new("e\u{301}");
    accent.set_style(
        Style::new()
            .fg(Color::LightRed)
            .underline_color(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut emoji = Cell::new("👩🏽‍💻");
    emoji.set_style(
        Style::new()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::ITALIC),
    );
    assert_eq!(buffer[(3, 4)], accent);
    assert_eq!(buffer[(4, 4)], emoji);
    assert_eq!(buffer[(5, 4)], Cell::EMPTY);
    assert_eq!(buffer[(6, 4)], Cell::new("x"));
}

#[test]
fn zero_width_graphemes_are_preserved_without_advancing_past_the_edge() {
    let area = Rect::new(0, 0, 3, 1);
    let mut buffer = Buffer::empty(area);
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("\u{200b}界\u{200b}x\u{200b}", ContentStyle::new())
        .unwrap();
    rect.text("\u{200b}", ContentStyle::new().red()).unwrap();
    assert_eq!(buffer[(0, 0)].symbol(), "\u{200b}界\u{200b}");
    assert_eq!(buffer[(1, 0)], Cell::EMPTY);
    assert_eq!(buffer[(2, 0)].symbol(), "x\u{200b}");
}

#[test]
fn empty_operations_do_not_visit_spans_or_touch_cells() {
    let area = Rect::new(0, 0, 1, 1);
    let mut buffer = Buffer::filled(area, dirty_cell());
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.next_line().unwrap();
    rect.text("", ContentStyle::new()).unwrap();
    rect.spaces(0, ContentStyle::new()).unwrap();
    rect.ascii(format_args!(""), ContentStyle::new()).unwrap();
    rect.styled_text("", std::iter::from_fn(|| panic!("visited empty spans")))
        .unwrap();
    assert_eq!(buffer[(0, 0)], dirty_cell());
}

#[test]
fn buffer_diff_uses_surface_widths_including_halfwidth_kana() {
    let kana_width = 2;
    let area = Rect::new(0, 0, kana_width + 7, 1);
    let before = Buffer::empty(area);
    let mut buffer = before.clone();
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("ｶﾞxلا\u{17d8}!", ContentStyle::new()).unwrap();
    let changes: Vec<_> = before
        .diff_iter(&buffer)
        .map(|(x, _, cell)| (x, cell.symbol()))
        .collect();
    assert_eq!(
        changes,
        [
            (0, "ｶﾞ"),
            (kana_width, "x"),
            (kana_width + 1, "ل"),
            (kana_width + 2, "ا"),
            (kana_width + 3, "\u{17d8}"),
            (kana_width + 6, "!")
        ]
    );
    assert_eq!(buffer[(0, 0)].cell_width(), kana_width);
    assert_eq!(buffer[(kana_width + 3, 0)].cell_width(), 3);
    assert_eq!(buffer[(kana_width + 4, 0)], Cell::EMPTY);
    assert_eq!(buffer[(kana_width + 5, 0)], Cell::EMPTY);
}

#[test]
fn overwriting_wide_glyphs_clears_their_entire_footprints() {
    let area = Rect::new(2, 1, 8, 1);
    let mut buffer = Buffer::filled(Rect::new(0, 0, 12, 3), dirty_cell());
    {
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        rect.text("a界b界cd", ContentStyle::new().on_blue())
            .unwrap();
        rect.move_to_column(2).unwrap();
        rect.text("X", ContentStyle::new()).unwrap();
        rect.move_to_column(3).unwrap();
        rect.ascii(format_args!("YZ"), ContentStyle::new()).unwrap();
        rect.move_to_column(6).unwrap();
        rect.text("界", ContentStyle::new()).unwrap();
        rect.move_to_column(7).unwrap();
        rect.spaces(1, ContentStyle::new().on_dark_green()).unwrap();
    }
    assert_eq!(buffer[(2, 1)].symbol(), "a");
    assert_eq!(buffer[(2, 1)].bg, Color::LightBlue);
    for (column, symbol) in [(1, " "), (2, "X"), (3, "Y"), (4, "Z"), (5, " "), (6, " ")] {
        assert_eq!(buffer[(2 + column, 1)], Cell::new(symbol));
    }
    let mut expected = Cell::EMPTY;
    expected.set_bg(Color::Green);
    assert_eq!(buffer[(9, 1)], expected);
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}

#[test]
fn formatting_streams_padding_and_numbers_with_complete_styles() {
    let area = Rect::new(2, 3, 7, 1);
    let mut buffer = Buffer::filled(area, dirty_cell());
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.ascii(
        format_args!("{:>5}/{}", 123, 9),
        ContentStyle::new().italic().green(),
    )
    .unwrap();
    for (x, ch) in (2..9).zip("  123/9".chars()) {
        let mut expected = Cell::from(ch);
        expected.set_style(
            Style::new()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::ITALIC),
        );
        assert_eq!(buffer[(x, 3)], expected);
    }
}

#[test]
fn colors_preserve_palette_intensity_rgb_and_indexed_values() {
    for (source, expected) in [
        (CrosstermColor::Reset, Color::Reset),
        (CrosstermColor::Black, Color::Black),
        (CrosstermColor::DarkGrey, Color::DarkGray),
        (CrosstermColor::Red, Color::LightRed),
        (CrosstermColor::DarkRed, Color::Red),
        (CrosstermColor::Green, Color::LightGreen),
        (CrosstermColor::DarkGreen, Color::Green),
        (CrosstermColor::Yellow, Color::LightYellow),
        (CrosstermColor::DarkYellow, Color::Yellow),
        (CrosstermColor::Blue, Color::LightBlue),
        (CrosstermColor::DarkBlue, Color::Blue),
        (CrosstermColor::Magenta, Color::LightMagenta),
        (CrosstermColor::DarkMagenta, Color::Magenta),
        (CrosstermColor::Cyan, Color::LightCyan),
        (CrosstermColor::DarkCyan, Color::Cyan),
        (CrosstermColor::White, Color::White),
        (CrosstermColor::Grey, Color::Gray),
        (
            CrosstermColor::Rgb { r: 3, g: 4, b: 5 },
            Color::Rgb(3, 4, 5),
        ),
        (CrosstermColor::AnsiValue(123), Color::Indexed(123)),
    ] {
        let area = Rect::new(0, 0, 2, 1);
        let mut buffer = Buffer::filled(area, dirty_cell());
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        rect.text(
            "x",
            ContentStyle::new()
                .with(source)
                .on(source)
                .underline(source),
        )
        .unwrap();
        rect.text("y", ContentStyle::new()).unwrap();
        assert_eq!(buffer[(0, 0)].fg, expected);
        assert_eq!(buffer[(0, 0)].bg, expected);
        assert_eq!(buffer[(0, 0)].underline_color, expected);
        assert_eq!(buffer[(1, 0)], Cell::new("y"));
    }
}

#[test]
fn attribute_removals_and_underline_variants_are_projected_to_modifiers() {
    let area = Rect::new(0, 0, 2, 1);
    let mut buffer = Buffer::filled(area, dirty_cell());
    let mut style = ContentStyle::new();
    for attribute in [
        Attribute::Bold,
        Attribute::Dim,
        Attribute::Italic,
        Attribute::Undercurled,
        Attribute::SlowBlink,
        Attribute::RapidBlink,
        Attribute::Reverse,
        Attribute::Hidden,
        Attribute::CrossedOut,
        Attribute::Framed,
    ] {
        style.attributes.set(attribute);
    }
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("x", style).unwrap();
    for attribute in [
        Attribute::NormalIntensity,
        Attribute::NoItalic,
        Attribute::NoUnderline,
        Attribute::NoBlink,
        Attribute::NoReverse,
        Attribute::NoHidden,
        Attribute::NotCrossedOut,
    ] {
        style.attributes.set(attribute);
    }
    rect.text("y", style).unwrap();
    assert_eq!(buffer[(0, 0)].modifier, Modifier::all());
    assert_eq!(buffer[(1, 0)], Cell::new("y"));
}

#[test]
fn prompt_status_and_preview_paint_into_shared_buffer_regions() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 14, 7), dirty_cell());
    let chars = PickerChars::new();
    let mut state = PromptState::new(Default::default());
    state.apply(PromptEvent::Reset("界abc".into()));
    let mut prompt = Prompt::new(&mut state, &chars);
    prompt
        .draw(&mut RatatuiSurface::new(
            &mut buffer,
            Rect::new(6, 8, 12, 1),
        ))
        .unwrap();
    StatusLine::new(&chars)
        .draw(&mut RatatuiSurface::new(
            &mut buffer,
            Rect::new(6, 9, 12, 1),
        ))
        .unwrap();

    let mut preview = crate::preview::Buffer::new();
    preview.push_styled_str("e", ContentStyle::new().red());
    preview.push_styled_str("\u{301}👩", ContentStyle::new().blue());
    preview.push_styled_str("🏽‍💻", ContentStyle::new().green());
    let pane = preview.into_pane();
    crate::preview::draw(
        &mut RatatuiSurface::new(&mut buffer, Rect::new(6, 10, 12, 3)),
        Content::Ready(&pane),
        None,
        '…',
    )
    .unwrap();

    assert_eq!(buffer[(6, 8)].symbol(), ">");
    assert_eq!(buffer[(8, 8)].symbol(), "界");
    assert_eq!(buffer[(10, 8)].symbol(), "a");
    assert_eq!(buffer[(17, 8)], Cell::EMPTY);
    for (x, ch) in (6..11).zip("  0/0".chars()) {
        assert_eq!(buffer[(x, 9)].symbol(), ch.to_string());
        assert_eq!(buffer[(x, 9)].modifier, Modifier::ITALIC);
    }
    assert_eq!(buffer[(6, 10)].symbol(), "e\u{301}");
    assert_eq!(buffer[(6, 10)].fg, Color::LightRed);
    assert_eq!(buffer[(7, 10)].symbol(), "👩🏽‍💻");
    assert_eq!(buffer[(7, 10)].fg, Color::LightBlue);
    assert_eq!(buffer[(8, 10)], Cell::EMPTY);
    assert_eq!(buffer[(17, 10)], Cell::EMPTY);
    for y in 11..13 {
        for x in 6..18 {
            assert_eq!(buffer[(x, y)], Cell::EMPTY);
        }
    }
    assert_outside_unchanged(&buffer, Rect::new(6, 8, 12, 5), &dirty_cell());
}

#[test]
fn match_list_paints_unicode_highlights_and_full_row_padding() {
    let mut driver = DriverOptions::new()
        .query("e")
        .highlight_line(true)
        .driver(StrRenderer);
    driver.push_batch(["界e\u{301}"]);
    driver.filter().unwrap();
    driver
        .list_state
        .layout
        .resize(driver.engine.snapshot(), 1, &driver.list_state.config);

    let area = Rect::new(3, 4, 8, 1);
    let mut buffer = Buffer::filled(Rect::new(2, 3, 10, 3), dirty_cell());
    driver
        .list_state
        .draw_items(
            &driver.engine,
            &mut RatatuiSurface::new(&mut buffer, area),
            &driver.chars,
            |_| true,
        )
        .unwrap();
    assert_eq!(buffer[(3, 4)].symbol(), driver.chars.hover.to_string());
    assert_eq!(buffer[(4, 4)].symbol(), driver.chars.queued.to_string());
    assert_eq!(buffer[(5, 4)].symbol(), "界");
    assert_eq!(buffer[(7, 4)].symbol(), "e\u{301}");
    assert_eq!(buffer[(7, 4)].fg, Color::LightCyan);
    for x in 7..11 {
        assert_eq!(buffer[(x, 4)].bg, Color::DarkGray);
        assert_eq!(buffer[(x, 4)].modifier, Modifier::BOLD);
    }
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}
