use ratatui_core::style::Color;

use super::*;
use crate::{
    PickerChars,
    preview::Content,
    render::StrRenderer,
    session::{
        ModelOptions,
        tests::{Queued, frame_options},
    },
};

fn dirty_cell() -> Cell {
    let mut cell = Cell::new(".");
    cell.set_style(
        RatatuiStyle::new()
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
        rect.text("a", Style::new()).unwrap();
        rect.move_to_column(1).unwrap();
        rect.text("b", Style::new()).unwrap();
        rect.next_line().unwrap();
        rect.text("123456", Style::new()).unwrap();
        rect.next_line().unwrap();
        rect.move_to(0, 0).unwrap();
        rect.text("c", Style::new()).unwrap();
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
fn construction_requires_containment_even_for_empty_rectangles() {
    for area in [
        Rect::new(6, 8, 10, 2),
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
fn clears_reset_only_the_assigned_cells_and_spaces_paint_their_style() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 10, 5), dirty_cell());
    let area = Rect::new(7, 8, 6, 3);
    {
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        rect.move_to(4, 1).unwrap();
        rect.clear_line().unwrap();
        rect.text("x", Style::new()).unwrap();
        rect.next_line().unwrap();
        rect.spaces(3, Style::new().bg(PickerColor::DarkRed))
            .unwrap();
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
    rect.text("y", Style::new()).unwrap();
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
            Style::new().fg(PickerColor::White).apply(""),
            Style::new()
                .fg(PickerColor::Red)
                .bold()
                .underline_color(PickerColor::Cyan)
                .apply("e"),
            Style::new()
                .fg(PickerColor::Blue)
                .italic()
                .apply("\u{301}👩"),
            Style::new().fg(PickerColor::Green).apply("🏽‍"),
            Style::new().fg(PickerColor::Red).apply(""),
            Style::new().fg(PickerColor::Yellow).apply("💻"),
            Style::new().apply("x"),
        ]
        .into_iter(),
    )
    .unwrap();
    let mut accent = Cell::new("e\u{301}");
    accent.set_style(
        RatatuiStyle::new()
            .fg(Color::LightRed)
            .underline_color(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut emoji = Cell::new("👩🏽‍💻");
    emoji.set_style(
        RatatuiStyle::new()
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
    rect.text("\u{200b}界\u{200b}x\u{200b}", Style::new())
        .unwrap();
    rect.text("\u{200b}", Style::new().fg(PickerColor::Red))
        .unwrap();
    assert_eq!(buffer[(0, 0)].symbol(), "\u{200b}界\u{200b}");
    assert_eq!(buffer[(1, 0)], Cell::EMPTY);
    assert_eq!(buffer[(2, 0)].symbol(), "x\u{200b}");
}

#[test]
fn empty_operations_leave_cells_unchanged() {
    let area = Rect::new(0, 0, 1, 1);
    let mut buffer = Buffer::filled(area, dirty_cell());
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("", Style::new()).unwrap();
    rect.spaces(0, Style::new()).unwrap();
    rect.ascii(format_args!(""), Style::new()).unwrap();
    rect.styled_text("", std::iter::empty()).unwrap();
    assert_eq!(buffer[(0, 0)], dirty_cell());
}

#[test]
fn buffer_diff_uses_surface_widths_including_halfwidth_kana() {
    let kana_width = 2;
    let area = Rect::new(0, 0, kana_width + 7, 1);
    let before = Buffer::empty(area);
    let mut buffer = before.clone();
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("ｶﾞxلا\u{17d8}!", Style::new()).unwrap();
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
        rect.text("a界b界cd", Style::new().bg(PickerColor::Blue))
            .unwrap();
        rect.move_to_column(2).unwrap();
        rect.text("X", Style::new()).unwrap();
        rect.move_to_column(3).unwrap();
        rect.ascii(format_args!("YZ"), Style::new()).unwrap();
        rect.move_to_column(6).unwrap();
        rect.text("界", Style::new()).unwrap();
        rect.move_to_column(7).unwrap();
        rect.spaces(1, Style::new().bg(PickerColor::DarkGreen))
            .unwrap();
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
        Style::new().italic().fg(PickerColor::Green),
    )
    .unwrap();
    for (x, ch) in (2..9).zip("  123/9".chars()) {
        let mut expected = Cell::from(ch);
        expected.set_style(
            RatatuiStyle::new()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::ITALIC),
        );
        assert_eq!(buffer[(x, 3)], expected);
    }
}

#[test]
fn colors_preserve_palette_intensity_rgb_and_indexed_values() {
    for (source, expected) in [
        (PickerColor::Reset, Color::Reset),
        (PickerColor::Black, Color::Black),
        (PickerColor::DarkGrey, Color::DarkGray),
        (PickerColor::Red, Color::LightRed),
        (PickerColor::DarkRed, Color::Red),
        (PickerColor::Green, Color::LightGreen),
        (PickerColor::DarkGreen, Color::Green),
        (PickerColor::Yellow, Color::LightYellow),
        (PickerColor::DarkYellow, Color::Yellow),
        (PickerColor::Blue, Color::LightBlue),
        (PickerColor::DarkBlue, Color::Blue),
        (PickerColor::Magenta, Color::LightMagenta),
        (PickerColor::DarkMagenta, Color::Magenta),
        (PickerColor::Cyan, Color::LightCyan),
        (PickerColor::DarkCyan, Color::Cyan),
        (PickerColor::White, Color::White),
        (PickerColor::Grey, Color::Gray),
        (PickerColor::Rgb { r: 3, g: 4, b: 5 }, Color::Rgb(3, 4, 5)),
        (PickerColor::AnsiValue(123), Color::Indexed(123)),
    ] {
        assert_eq!(Color::from(source), expected);
        assert_eq!(PickerColor::from(expected), source);
        let area = Rect::new(0, 0, 2, 1);
        let mut buffer = Buffer::filled(area, dirty_cell());
        let mut rect = RatatuiSurface::new(&mut buffer, area);
        rect.text(
            "x",
            Style::new().fg(source).bg(source).underline_color(source),
        )
        .unwrap();
        rect.text("y", Style::new()).unwrap();
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
    let mut style = Style::new();
    for attribute in [
        Attributes::BOLD,
        Attributes::DIM,
        Attributes::ITALIC,
        Attributes::UNDERCURLED,
        Attributes::SLOW_BLINK,
        Attributes::RAPID_BLINK,
        Attributes::REVERSE,
        Attributes::HIDDEN,
        Attributes::CROSSED_OUT,
        Attributes::FRAMED,
    ] {
        style.attributes.insert(attribute);
    }
    let mut rect = RatatuiSurface::new(&mut buffer, area);
    rect.text("x", style).unwrap();
    for attribute in [
        Attributes::BOLD,
        Attributes::DIM,
        Attributes::ITALIC,
        Attributes::UNDERCURLED,
        Attributes::SLOW_BLINK,
        Attributes::RAPID_BLINK,
        Attributes::REVERSE,
        Attributes::HIDDEN,
        Attributes::CROSSED_OUT,
    ] {
        style.attributes.remove(attribute);
    }
    rect.text("y", style).unwrap();
    assert_eq!(buffer[(0, 0)].modifier, Modifier::all());
    assert_eq!(buffer[(1, 0)], Cell::new("y"));
}

#[test]
fn modifier_conversions_preserve_supported_flags_and_normalize_unsupported_attributes() {
    for modifier in Modifier::all()
        .iter()
        .chain([Modifier::empty(), Modifier::all()])
    {
        assert_eq!(Modifier::from(Attributes::from(modifier)), modifier);
    }
    let unsupported =
        Attributes::FRAKTUR | Attributes::FRAMED | Attributes::ENCIRCLED | Attributes::OVERLINED;
    assert_eq!(Modifier::from(unsupported), Modifier::empty());
    for underline in [
        Attributes::UNDERLINED,
        Attributes::DOUBLE_UNDERLINED,
        Attributes::UNDERCURLED,
        Attributes::UNDERDOTTED,
        Attributes::UNDERDASHED,
    ] {
        let converted = Modifier::from(underline | unsupported | Attributes::BOLD);
        assert_eq!(converted, Modifier::UNDERLINED | Modifier::BOLD);
        assert_eq!(
            Attributes::from(converted),
            Attributes::UNDERLINED | Attributes::BOLD,
        );
    }
}

#[test]
fn style_conversions_resolve_removals_and_preserve_optional_colors() {
    let source = RatatuiStyle {
        fg: Some(Color::Rgb(3, 4, 5)),
        bg: Some(Color::Reset),
        underline_color: Some(Color::Indexed(9)),
        add_modifier: Modifier::BOLD | Modifier::DIM | Modifier::ITALIC,
        sub_modifier: Modifier::BOLD | Modifier::UNDERLINED,
    };
    assert_eq!(
        Style::from(source),
        Style::new()
            .fg(PickerColor::Rgb { r: 3, g: 4, b: 5 })
            .bg(PickerColor::Reset)
            .underline_color(PickerColor::AnsiValue(9))
            .dim()
            .italic(),
    );
    assert_eq!(Style::from(RatatuiStyle::new()), Style::new());
    assert_eq!(
        Style::from(RatatuiStyle::reset()),
        Style::new()
            .fg(PickerColor::Reset)
            .bg(PickerColor::Reset)
            .underline_color(PickerColor::Reset),
    );
    for style in [
        Style::new(),
        Style::new().fg(PickerColor::Reset).reverse(),
        Style::new().bg(PickerColor::DarkRed).bold(),
        Style::new()
            .fg(PickerColor::Grey)
            .underline_color(PickerColor::Cyan)
            .underlined(),
    ] {
        assert_eq!(Style::from(RatatuiStyle::from(style)), style);
    }
    for style in [source, RatatuiStyle::new(), RatatuiStyle::reset()] {
        let mut expected = Cell::EMPTY;
        expected.set_style(style);
        let mut converted = Cell::EMPTY;
        converted.set_style(Style::from(style));
        assert_eq!(converted, expected);
    }
}

#[test]
fn span_conversions_borrow_both_borrowed_and_owned_content() {
    let source = String::from("prefix é界👩🏽‍💻 suffix");
    let style = Style::new()
        .fg(PickerColor::Cyan)
        .bg(PickerColor::Reset)
        .underline_color(PickerColor::AnsiValue(42))
        .bold();
    for text in [&source[..0], &source[7..source.len() - 7]] {
        let native = style.apply(text);
        let borrowed = Span::from(native);
        assert!(matches!(borrowed.content, std::borrow::Cow::Borrowed(_)));
        assert_eq!(borrowed.content.as_ptr(), text.as_ptr());
        assert_eq!(borrowed.style, RatatuiStyle::from(style));
        for span in [borrowed, Span::styled(text.to_owned(), style)] {
            let converted = StyledStr::from(&span);
            assert_eq!(converted, native);
            assert_eq!(converted.text.as_ptr(), span.content.as_ptr());
        }
    }
}

#[test]
fn preview_paints_split_graphemes_and_clears_its_buffer_region() {
    let mut buffer = Buffer::filled(Rect::new(5, 7, 14, 7), dirty_cell());
    let area = Rect::new(6, 10, 12, 3);
    let mut preview = crate::preview::Buffer::new();
    preview.push_styled_str("e", Style::new().fg(PickerColor::Red));
    preview.push_styled_str("\u{301}👩", Style::new().fg(PickerColor::Blue));
    preview.push_styled_str("🏽‍💻", Style::new().fg(PickerColor::Green));
    let pane = preview.into_pane();
    crate::preview::draw(
        &mut RatatuiSurface::new(&mut buffer, area),
        Content::Ready(&pane),
        None,
        '…',
    )
    .unwrap();

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
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}

#[test]
fn match_list_paints_unicode_highlights_and_full_row_padding() {
    let chars = PickerChars::new();
    let mut model = ModelOptions::new()
        .query("e")
        .highlight_line(true)
        .model(StrRenderer);
    model.push_batch(["界e\u{301}"]);
    model.filter().unwrap();
    let queued = StateCell::new(Some(model.matches().get(0).unwrap().id()));
    let mut session = model.session(Queued(&queued));
    let mut frame = session.prepare_frame(frame_options());

    let area = Rect::new(3, 4, 8, 1);
    let mut buffer = Buffer::filled(Rect::new(2, 3, 10, 3), dirty_cell());
    frame
        .match_list()
        .draw(&mut RatatuiSurface::new(&mut buffer, area))
        .unwrap();
    frame.complete();
    assert_eq!(buffer[(3, 4)].symbol(), chars.hover.to_string());
    assert_eq!(buffer[(4, 4)].symbol(), chars.queued.to_string());
    assert_eq!(buffer[(5, 4)].symbol(), "界");
    assert_eq!(buffer[(7, 4)].symbol(), "e\u{301}");
    assert_eq!(buffer[(7, 4)].fg, Color::LightCyan);
    for x in 7..11 {
        assert_eq!(buffer[(x, 4)].bg, Color::DarkGray);
        assert_eq!(buffer[(x, 4)].modifier, Modifier::BOLD);
    }
    assert_outside_unchanged(&buffer, area, &dirty_cell());
}
use std::cell::Cell as StateCell;
