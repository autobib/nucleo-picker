use std::time::Duration;

use super::{Layout, LayoutEvent};
use crate::{
    DriverOptions,
    rect::Area,
    render::StrRenderer,
    session::{
        Context, Dimensions, Exit, Geometry, Interface, Redraw,
        tests::{Select, TestTerminal, frame_options},
    },
};

struct EmptyPreview;

impl Interface<&'static str> for EmptyPreview {
    type State = ();
    type Output<'items> = Option<&'items &'static str>;
    const PREVIEW: bool = true;

    fn init(&mut self, _: &Context<'_, &'static str>) {}
    fn finish<'items>(
        &mut self,
        _: (),
        _: Exit,
        _: Context<'items, &'static str>,
    ) -> Self::Output<'items> {
        None
    }
}

fn dimensions((width, height): (u16, u16)) -> Dimensions {
    Dimensions { width, height }
}

fn render(redraw: Redraw, size: (u16, u16), interface: impl Interface<&'static str>) -> String {
    let mut driver = DriverOptions::new().driver(StrRenderer);
    let mut session = driver.session(interface, Duration::ZERO);
    let preview = session.events().preview_enabled();
    let mut layout = Layout::new(dimensions(size), false, preview.then_some(0.5));
    let mut initial = session.prepare_frame(frame_options());
    initial.resize(layout.geometry(dimensions(size), preview));
    initial.complete();
    let mut frame = session.prepare_frame(frame_options());
    assert!(!frame.changed());
    let mut terminal = TestTerminal::default();
    frame.draw(&mut terminal, redraw).unwrap();
    assert!(!frame.changed());
    frame.complete();
    String::from_utf8(terminal.output).unwrap()
}

#[test]
fn host_repaints_respect_component_boundaries_without_changing_the_frame() {
    for preview_enabled in [false, true] {
        for (flags, screen_clear) in [
            ([true, false, false, false], false),
            ([false, true, false, false], false),
            ([false, false, true, false], false),
            ([false, false, false, true], false),
            ([true, true, true, false], true),
        ] {
            let [prompt, list, status, preview] = flags;
            let redraw = Redraw {
                prompt,
                list,
                status,
                preview,
            };
            let output = if preview_enabled {
                render(redraw, (20, 4), EmptyPreview)
            } else {
                render(redraw, (20, 4), Select)
            };
            assert_eq!(
                [
                    output.contains('>'),
                    output.contains("\x1b[1;1H"),
                    output.contains("0/0"),
                    output.contains('╭'),
                ],
                [
                    prompt,
                    list,
                    status,
                    preview_enabled && (preview || screen_clear)
                ],
                "{flags:?}, preview={preview_enabled}",
            );
            assert_eq!(output.contains("\x1b[2J"), screen_clear);
            assert_eq!(
                output.contains("\x1b[K"),
                !preview_enabled && !screen_clear && (prompt || list),
            );
            assert!(output.starts_with("\x1b[?2026h"));
            assert!(output.ends_with("\x1b[4;3H\x1b[?2026l"));
        }
    }
}

#[test]
fn moved_and_empty_geometry_is_applied_before_painting() {
    let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
    let mut session = driver.session(Select, Duration::ZERO);
    for (width, height) in [(20, 8), (0, 8), (20, 0), (20, 8)] {
        let mut frame = session.prepare_frame(frame_options());
        frame.resize(Geometry {
            dimensions: Dimensions { width, height },
            prompt: Area {
                column: 3,
                row: 2,
                width: 10,
                height: 1,
            },
            ..Geometry::default()
        });
        assert!(frame.changed());
        let mut terminal = TestTerminal::default();
        frame.draw(&mut terminal, frame.redraw()).unwrap();
        if width == 0 || height == 0 {
            assert!(terminal.output.is_empty());
        } else {
            let output = String::from_utf8(terminal.output).unwrap();
            assert!(output.contains("\x1b[3;4H"));
            assert!(output.ends_with("\x1b[3;6H\x1b[?2026l"));
        }
        frame.complete();
        let idle = session.prepare_frame(frame_options());
        assert!(!idle.needs_render());
        idle.complete();
    }
}

#[test]
fn status_visibility_survives_resizing_in_both_orientations() {
    for reversed in [false, true] {
        let size = dimensions((20, 5));
        let mut layout = Layout::new(size, reversed, Some(0.4));
        let initial = layout.geometry(size, true);
        layout.handle(LayoutEvent::ToggleStatus, true);
        assert!(layout.layout_changed());
        let hidden = layout.geometry(size, true);
        assert!(hidden.status.is_empty());
        assert_eq!(hidden.list.height, 4);
        assert_eq!(hidden.list.row, u16::from(reversed));
        assert_eq!(
            (hidden.prompt, hidden.preview),
            (initial.prompt, initial.preview)
        );
        assert!(!layout.layout_changed());
        assert!(layout.geometry(dimensions((30, 1)), true).status.is_empty());
        layout.handle(LayoutEvent::ToggleStatus, true);
        assert_eq!(layout.geometry(size, true), initial);
    }
}

#[test]
fn preview_ratios_round_clamp_and_round_trip_through_small_sizes() {
    for (width, ratio, expected) in [
        (101, 0.5, 51),
        (101, 0.4, 40),
        (101, 0.0, 3),
        (101, 1.0, 98),
        (6, 0.0, 3),
        (u16::MAX, 0.5, 32768),
    ] {
        let size = dimensions((width, 5));
        let mut layout = Layout::new(size, false, Some(ratio));
        let initial = layout.geometry(size, true);
        assert_eq!(initial.preview.width, expected);
        assert_eq!(initial.preview.column, width - expected);
        for small in [(0, 0), (5, 5), (101, 2)] {
            let geometry = layout.geometry(dimensions(small), true);
            assert!(geometry.preview.is_empty());
            assert_eq!(geometry.list.width, small.0);
        }
        assert_eq!(layout.geometry(size, true), initial);
    }
}

#[test]
fn divider_moves_from_displayed_width_and_survives_hiding_and_resize() {
    use LayoutEvent::{MoveDividerLeft as Left, MoveDividerRight as Right};

    let size = dimensions((101, 5));
    let mut layout = Layout::new(size, false, Some(0.5));
    for (event, expected) in [
        (Right(1), 50),
        (Left(1), 51),
        (Left(u16::MAX), 98),
        (Right(u16::MAX), 3),
        (Left(1), 4),
    ] {
        layout.handle(event, true);
        assert_eq!(layout.geometry(size, true).preview.width, expected);
    }
    layout.handle(LayoutEvent::TogglePreview, false);
    let hidden = layout.geometry(size, false);
    assert!(hidden.preview.is_empty());
    assert_eq!(hidden.list.width, 101);
    layout.handle(Left(20), false);
    assert!(!layout.layout_changed());
    assert!(
        layout
            .geometry(dimensions((202, 5)), false)
            .preview
            .is_empty()
    );
    layout.handle(LayoutEvent::TogglePreview, true);
    assert_eq!(layout.geometry(dimensions((202, 5)), true).preview.width, 8);

    let mut layout = Layout::new(dimensions((20, 5)), false, Some(0.01));
    layout.handle(Right(1), true);
    assert!(!layout.layout_changed());
    assert_eq!(
        layout.geometry(dimensions((1000, 5)), true).preview.width,
        10
    );
}

#[test]
fn undersized_previews_render_like_a_picker_without_preview() {
    for size in [(0, 4), (1, 1), (5, 4), (20, 2), (20, 0)] {
        assert_eq!(
            render(Redraw::FULL, size, EmptyPreview),
            render(Redraw::FULL, size, Select)
        );
    }
    assert!(render(Redraw::FULL, (6, 3), EmptyPreview).contains("\x1b[1;4H\x1b[4G╭─╮"));
}
