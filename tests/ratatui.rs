#![cfg(feature = "ratatui")]

use std::{cell::RefCell, num::NonZero, time::Instant};

use nucleo_picker::{
    draw::ratatui::RatatuiSurface,
    interface::Interface,
    preview::{BoundaryChars, Content, Pane},
    render::StrRenderer,
    session::{Components, Context, Dimensions, Exit, FrameOptions, Model, ModelOptions},
};
use ratatui_core::{
    backend::TestBackend,
    buffer::{Buffer, Cell as BufferCell},
    layout::Rect,
    style::{Color, Modifier, Style},
    terminal::Terminal,
    text::Line,
    widgets::Widget,
};
use ratatui_widgets::{block::Block, borders::BorderType};

#[derive(Default)]
struct Probe {
    resizes: RefCell<Vec<Option<Dimensions>>>,
    error_on_resize: bool,
}

impl Interface<&'static str> for &Probe {
    type State = Pane;
    type Output<'items> = Option<&'items &'static str>;

    const PREVIEW: bool = true;

    fn init(&mut self, _: &Context<'_, &'static str>) -> Pane {
        let mut pane = Pane::new();
        pane.push_text("preview");
        pane
    }

    fn preview_content<'a>(&'a self, pane: &'a Pane) -> Content<'a> {
        Content::Ready(pane)
    }

    fn preview_resize(&mut self, pane: &mut Pane, size: Option<Dimensions>) {
        self.resizes.borrow_mut().push(size);
        pane.set_err(self.error_on_resize);
    }

    fn finish<'items>(
        &mut self,
        _: Pane,
        _: Exit,
        _: Context<'items, &'static str>,
    ) -> Self::Output<'items> {
        None
    }
}

fn model() -> Model<&'static str, StrRenderer> {
    let mut model = ModelOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
        .list_reversed(true)
        .model(StrRenderer);
    model.push_batch(["ｶﾞalpha", "ｶﾞbeta"]);
    model.update_query("ｶﾞ");
    model.filter().unwrap();
    model
}

fn contents(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
        .map(|position| buffer[position].symbol())
        .collect()
}

#[test]
fn widgets_compose_in_offset_regions_and_repaint_unchanged_frames() {
    let mut model = model();
    let probe = Probe::default();
    let mut session = model.session(&probe);
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    let mut host = Buffer::filled(Rect::new(0, 0, 40, 12), BufferCell::new("."));
    Line::from("Application").render(Rect::new(1, 1, 15, 1), &mut host);
    let prompt = Rect::new(5, 3, 12, 1);
    let status = Rect::new(5, 4, 12, 1);
    let list = Rect::new(5, 5, 12, 3);
    let preview = Rect::new(20, 3, 12, 5);
    let preview_block = Block::bordered().border_type(BorderType::Double);
    let preview_content = preview_block.inner(preview);
    let cursor_column = 4;
    let mut first = None;

    for pass in 0..2 {
        let mut prepared = session.prepare_frame(FrameOptions::new(Instant::now()));
        assert_eq!(prepared.changed(), pass == 0);
        let rendered = terminal
            .draw(|frame| {
                frame.buffer_mut().clone_from(&host);
                frame.render_widget(prepared.prompt(), prompt);
                frame.render_widget(prepared.match_list(), list);
                frame.render_widget(prepared.status(), status);
                frame.render_widget(preview_block.clone(), preview);
                frame.render_widget(prepared.preview(), preview_content);
                let cursor = prepared.prompt().cursor().unwrap();
                assert_eq!((cursor.column, cursor.row), (cursor_column, 0));
                if pass == 0 {
                    frame.set_cursor_position((prompt.x + cursor.column, prompt.y + cursor.row));
                }
            })
            .unwrap();

        assert!(contents(rendered.buffer, prompt).starts_with("> ｶﾞ"));
        assert!(contents(rendered.buffer, list).contains("alpha"));
        assert!(contents(rendered.buffer, list).contains("beta"));
        assert!(contents(rendered.buffer, status).contains("2/2"));
        assert!(contents(rendered.buffer, preview).contains("preview"));
        assert_eq!(rendered.buffer[(preview.x, preview.y)].symbol(), "╔");
        assert_eq!(
            rendered.buffer[(preview_content.x, preview_content.y)].symbol(),
            "p",
        );
        assert_eq!(
            *probe.resizes.borrow(),
            [Some(Dimensions {
                width: preview_content.width,
                height: preview_content.height,
            })],
        );
        for y in host.area.y..host.area.bottom() {
            for x in host.area.x..host.area.right() {
                if ![prompt, status, list, preview]
                    .iter()
                    .any(|area| area.contains((x, y).into()))
                {
                    assert_eq!(rendered.buffer[(x, y)], host[(x, y)], "at ({x}, {y})");
                }
            }
        }
        if let Some(first) = &first {
            assert_eq!(rendered.buffer, first);
        } else {
            first = Some(rendered.buffer.clone());
        }
        assert_eq!(terminal.backend().cursor_visible(), pass == 0);
        if pass == 0 {
            assert_eq!(
                terminal.backend().cursor_position(),
                (prompt.x + cursor_column, prompt.y).into()
            );
        }
        prepared.complete();
    }
}

#[test]
fn prepared_preview_widget_and_direct_drawing_replace_dirty_cells_with_a_styled_border() {
    let mut model = model();
    let probe = Probe {
        error_on_resize: true,
        ..Probe::default()
    };
    let mut session = model.session(&probe);
    let mut dirty = BufferCell::new("x");
    dirty.set_style(
        Style::new()
            .fg(Color::Magenta)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let mut host = Buffer::filled(Rect::new(4, 6, 30, 10), dirty);
    let area = Rect::new(7, 8, 16, 5);
    host[(8, 8)].set_symbol("界");
    host[(8, 9)].set_symbol("界");
    host[(15, 10)].set_symbol("界");
    let size = Dimensions {
        width: area.width,
        height: area.height,
    };
    for _ in 0..2 {
        let mut prepared = session.prepare_frame(FrameOptions::new(Instant::now()));
        let mut direct = host.clone();
        prepared
            .preview()
            .border(BoundaryChars::ascii())
            .resize(size);
        prepared
            .preview()
            .border(BoundaryChars::ascii())
            .draw(&mut RatatuiSurface::new(&mut direct, area))
            .unwrap();
        let mut widget = host.clone();
        prepared
            .preview()
            .border(BoundaryChars::ascii())
            .render(area, &mut widget);
        assert_eq!(widget, direct);
        assert_eq!(
            contents(&widget, Rect::new(area.x, area.y, area.width, 1)),
            "+- Error ------+",
        );
        assert_eq!(
            contents(&widget, Rect::new(area.x, area.y + 1, area.width, 1)),
            "|preview       |",
        );
        assert_eq!(
            *probe.resizes.borrow(),
            [Some(Dimensions {
                width: 14,
                height: 3
            })],
        );
        for y in host.area.y..host.area.bottom() {
            for x in host.area.x..host.area.right() {
                let cell = &widget[(x, y)];
                if !area.contains((x, y).into()) {
                    assert_eq!(cell, &host[(x, y)], "at ({x}, {y})");
                } else if x == area.x
                    || x == area.right() - 1
                    || y == area.y
                    || y == area.bottom() - 1
                {
                    assert_eq!(cell.fg, Color::Red);
                    assert_eq!(cell.bg, Color::Reset);
                    assert!(cell.modifier.is_empty());
                } else if y == area.y + 1 && x < area.x + 8 {
                    assert_eq!(cell.fg, Color::Reset);
                    assert_eq!(cell.bg, Color::Reset);
                    assert!(cell.modifier.is_empty());
                } else {
                    assert_eq!(cell, &BufferCell::EMPTY);
                }
            }
        }
        prepared.complete();
    }
    session.events().set_active_components(Components::NONE);
    let mut hidden = session.prepare_frame(FrameOptions::new(Instant::now()));
    let mut buffer = host.clone();
    hidden
        .preview()
        .border(BoundaryChars::ascii())
        .render(area, &mut buffer);
    assert_eq!(buffer, host);
    assert_eq!(probe.resizes.borrow().last(), Some(&None));
    hidden.complete();
}

#[test]
fn widgets_deliver_empty_and_hidden_preview_transitions_without_painting() {
    let mut model = model();
    let probe = Probe::default();
    let mut session = model.session(&probe);
    let host = Buffer::filled(Rect::new(4, 6, 30, 10), BufferCell::new("."));
    let mut expected = Vec::new();

    for (width, height, visible, size) in [
        (12, 5, true, Some((12, 5))),
        (7, 1, true, Some((7, 1))),
        (0, 5, true, Some((0, 5))),
        (12, 0, true, Some((12, 0))),
        (12, 5, false, None),
        (12, 5, true, Some((12, 5))),
    ] {
        session.events().set_active_components(Components {
            preview: visible,
            ..Components::ALL
        });
        let mut prepared = session.prepare_frame(FrameOptions::new(Instant::now()));
        let mut buffer = host.clone();
        let area = Rect::new(7, 8, width, height);
        prepared.preview().render(area, &mut buffer);
        prepared.preview().render(area, &mut buffer);
        expected.push(size.map(|(width, height)| Dimensions { width, height }));
        assert_eq!(*probe.resizes.borrow(), expected);
        if area.is_empty() || !visible {
            assert_eq!(buffer, host);
        } else {
            assert!(contents(&buffer, area).contains("preview"));
            assert_eq!(buffer[(area.x, area.y)].symbol(), "p");
        }
        prepared.complete();
    }

    let mut prepared = session.prepare_frame(FrameOptions::new(Instant::now()));
    for empty in [Rect::new(7, 8, 0, 5), Rect::new(7, 8, 12, 0)] {
        let mut buffer = host.clone();
        prepared.prompt().render(empty, &mut buffer);
        prepared.match_list().render(empty, &mut buffer);
        prepared.status().render(empty, &mut buffer);
        assert_eq!(buffer, host);
        assert_eq!(prepared.prompt().cursor(), None);
    }
    prepared.complete();
}
