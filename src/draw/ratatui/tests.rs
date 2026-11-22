use crossterm::style::{Color as CrosstermColor, Stylize};
use ratatui_core::style::Color;

use super::*;
use crate::{
    PickerChars,
    component::Component,
    preview::{BoundaryChars, Content},
    prompt::{Prompt, PromptEvent, PromptState},
    rect::Area,
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
    assert!(RatatuiRect::new(&mut buffer, Rect::new(7, 8, 0, 3)).is_none());
    assert!(RatatuiRect::new(&mut buffer, Rect::new(7, 8, 6, 0)).is_none());
    {
        let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
        assert_eq!(rect.width().get(), 6);
        assert_eq!(rect.height().get(), 3);
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
    RatatuiRect::new(&mut buffer, Rect::new(6, 8, 10, 2));
}

#[test]
#[should_panic(expected = "drawing cursor is invalid")]
fn advancing_past_the_last_row_invalidates_the_cursor() {
    let area = Rect::new(0, 0, 1, 1);
    let mut buffer = Buffer::empty(area);
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
    rect.next_line().unwrap();
    rect.text("x", ContentStyle::new()).unwrap();
}

#[test]
fn clears_reset_only_the_assigned_cells_and_spaces_paint_their_style() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 10, 5), dirty_cell());
    let area = Rect::new(7, 8, 6, 3);
    {
        let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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

    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
    rect.next_line().unwrap();
    rect.text("", ContentStyle::new()).unwrap();
    rect.spaces(0, ContentStyle::new()).unwrap();
    rect.ascii(format_args!(""), ContentStyle::new()).unwrap();
    rect.styled_text("", std::iter::from_fn(|| panic!("visited empty spans")))
        .unwrap();
    assert_eq!(buffer[(0, 0)], dirty_cell());
}

#[test]
fn grapheme_width_is_bounded_by_utf8_length() {
    let check = |text: &str| {
        for (_, grapheme) in UnicodeProcessor::grapheme_indices(text) {
            assert!(
                RatatuiRect::grapheme_width(grapheme) <= grapheme.len(),
                "width exceeds UTF-8 byte length: {grapheme:?}"
            );
        }
    };

    for ch in ('\0'..=char::MAX).filter(|ch| !ch.is_control()) {
        check(ch.encode_utf8(&mut [0; 4]));
    }
    for text in [
        "e\u{301}",
        "\u{200b}",
        "\u{17d8}",
        "ｶﾞ",
        "ﾊﾟ",
        "aﾞ",
        "界ﾞﾟ",
        "\u{ff9e}\u{ff9f}\u{ff9e}",
        "1\u{fe0f}\u{20e3}",
        "©\u{fe0f}",
        "“\u{fe01}",
        "👩🏽‍💻",
        "🇨🇦",
        "لا",
        "\u{1780}\u{17d2}\u{1780}",
    ] {
        check(text);
    }
}

#[test]
fn buffer_diff_uses_ratatui_widths_including_halfwidth_kana() {
    let kana_width = "ｶﾞ".cell_width();
    let area = Rect::new(0, 0, kana_width + 7, 1);
    let before = Buffer::empty(area);
    let mut buffer = before.clone();
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
        let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
        let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    let mut rect = RatatuiRect::new(&mut buffer, area).unwrap();
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
    state.apply(
        PromptEvent::Reset("界abc".into()),
        RatatuiRect::grapheme_width,
    );
    let mut prompt = Prompt::new(&mut state, &chars);
    prompt.resize(Area {
        width: 12,
        height: 1,
        ..Area::default()
    });
    prompt
        .draw(&mut RatatuiRect::new(&mut buffer, Rect::new(6, 8, 12, 1)).unwrap())
        .unwrap();
    StatusLine::new(&chars)
        .draw(&mut RatatuiRect::new(&mut buffer, Rect::new(6, 9, 12, 1)).unwrap())
        .unwrap();

    let mut preview = crate::preview::Buffer::new();
    preview.push_styled_str("e", ContentStyle::new().red());
    preview.push_styled_str("\u{301}👩", ContentStyle::new().blue());
    preview.push_styled_str("🏽‍💻", ContentStyle::new().green());
    let pane = preview.into_pane();
    let borders = BoundaryChars::default();
    crate::preview::draw(
        &mut RatatuiRect::new(&mut buffer, Rect::new(6, 10, 12, 3)).unwrap(),
        Content::Ready(&pane),
        borders,
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
    assert_eq!(buffer[(6, 10)], Cell::from(borders.top_left));
    assert_eq!(buffer[(6, 12)], Cell::from(borders.bottom_left));
    assert_eq!(buffer[(7, 11)].symbol(), "e\u{301}");
    assert_eq!(buffer[(7, 11)].fg, Color::LightRed);
    assert_eq!(buffer[(8, 11)].symbol(), "👩🏽‍💻");
    assert_eq!(buffer[(8, 11)].fg, Color::LightBlue);
    assert_eq!(buffer[(9, 11)], Cell::EMPTY);
    assert_eq!(buffer[(17, 11)], Cell::from(borders.vertical));
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
            &mut RatatuiRect::new(&mut buffer, area).unwrap(),
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
