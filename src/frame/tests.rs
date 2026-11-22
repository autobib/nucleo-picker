use std::{cell::RefCell, io};

use super::Layout;
use crate::{
    Terminal,
    component::tests::TestSurface,
    interface::Interface,
    preview::{BoundaryChars, Content, Pane},
    rect::Area,
    render::StrRenderer,
    session::{
        Components, Context, Dimensions, DriverOptions, Exit,
        tests::{Select, TestTerminal, frame_options},
    },
};

#[derive(Default)]
struct EmptyPreview<'a>(Option<&'a RefCell<Vec<Option<Dimensions>>>>);

impl Interface<&'static str> for EmptyPreview<'_> {
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

    fn preview_resize(&mut self, _: &mut (), size: Option<Dimensions>) {
        if let Some(resizes) = self.0 {
            resizes.borrow_mut().push(size);
        }
    }
}

struct EmptyTerminal;

impl io::Write for EmptyTerminal {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("empty viewport must not write");
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("empty viewport must not flush");
    }
}

impl Terminal for EmptyTerminal {
    fn init(&mut self) -> io::Result<()> {
        unreachable!()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        unreachable!()
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        unreachable!()
    }

    fn begin_render(&mut self) -> io::Result<()> {
        panic!("empty viewport must not begin rendering");
    }

    fn end_render(&mut self) -> io::Result<()> {
        panic!("empty viewport must not end rendering");
    }
}

fn dimensions((width, height): (u16, u16)) -> Dimensions {
    Dimensions { width, height }
}

fn render(
    redraw: Components,
    clear_screen: bool,
    size: (u16, u16),
    interface: impl Interface<&'static str>,
) -> String {
    let mut driver = DriverOptions::new().driver(StrRenderer);
    let mut session = driver.session(interface);
    let layout = Layout::new(dimensions(size), false, 0.5, session.active_components());
    let mut initial = session.prepare_frame(frame_options());
    super::draw(
        &layout,
        &mut initial,
        &mut TestTerminal::default(),
        Components::ALL,
        true,
        BoundaryChars::new(),
    )
    .unwrap();
    initial.complete();
    let mut frame = session.prepare_frame(frame_options());
    assert!(!frame.changed());
    let mut terminal = TestTerminal::default();
    super::draw(
        &layout,
        &mut frame,
        &mut terminal,
        redraw,
        clear_screen,
        BoundaryChars::new(),
    )
    .unwrap();
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
            ([true, true, true, false], false),
            ([true, true, true, true], false),
            ([false, false, false, false], true),
        ] {
            let [prompt, list, status, preview] = flags;
            let redraw = Components {
                prompt,
                match_list: list,
                status,
                preview,
            };
            let output = if preview_enabled {
                render(redraw, screen_clear, (20, 4), EmptyPreview::default())
            } else {
                render(redraw, screen_clear, (20, 4), Select)
            };
            assert_eq!(
                [
                    output.contains('>'),
                    output.contains("\x1b[1;1H"),
                    output.contains("0/0"),
                    output.contains('╭'),
                ],
                [
                    prompt || screen_clear,
                    list || screen_clear,
                    status || screen_clear,
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
fn moved_and_empty_layout_is_applied_while_painting() {
    let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
    let mut session = driver.session(Select);
    session.prepare_frame(frame_options()).complete();
    let mut layout = Layout::new(dimensions((20, 8)), false, 0.5, session.active_components());
    for (width, height) in [(20, 8), (0, 8), (20, 0), (20, 8)] {
        layout.resolve(Dimensions { width, height }, session.active_components());
        if width != 0 && height != 0 {
            layout.prompt = Area {
                column: 3,
                row: 2,
                width: 10,
                height: 1,
            };
        }
        let mut frame = session.prepare_frame(frame_options());
        assert!(!frame.changed());
        if width == 0 || height == 0 {
            super::draw(
                &layout,
                &mut frame,
                &mut EmptyTerminal,
                Components::ALL,
                true,
                BoundaryChars::new(),
            )
            .unwrap();
            assert!(frame.prompt().cursor().is_none());
        } else {
            let mut terminal = TestTerminal::default();
            super::draw(
                &layout,
                &mut frame,
                &mut terminal,
                Components::ALL,
                true,
                BoundaryChars::new(),
            )
            .unwrap();
            let output = String::from_utf8(terminal.output).unwrap();
            assert!(output.contains("\x1b[3;4H"));
            assert!(output.ends_with("\x1b[3;6H\x1b[?2026l"));
        }
        assert!(!frame.changed());
        frame.complete();
        let idle = session.prepare_frame(frame_options());
        assert!(!idle.changed());
        idle.complete();
    }
}

#[test]
fn hidden_and_empty_components_are_sized_without_repaint_requests() {
    let resizes = RefCell::new(Vec::new());
    let mut driver = DriverOptions::new().driver::<&str, _>(StrRenderer);
    let mut session = driver.session(EmptyPreview(Some(&resizes)));
    let mut layout = Layout::new(dimensions((20, 4)), false, 0.5, session.active_components());
    for (shown, size) in [
        (true, (20, 4)),
        (false, (20, 4)),
        (true, (20, 4)),
        (true, (0, 4)),
        (true, (20, 4)),
        (true, (20, 0)),
        (true, (20, 4)),
    ] {
        session.events().set_active_components(if shown {
            Components::ALL
        } else {
            Components::NONE
        });
        layout.resolve(dimensions(size), session.active_components());
        let nonempty = size.0 != 0 && size.1 != 0;
        let redraw = if shown && nonempty {
            Components::ALL
        } else {
            Components::NONE
        };
        let mut frame = session.prepare_frame(frame_options());
        let mut terminal = TestTerminal::default();
        super::draw(
            &layout,
            &mut frame,
            &mut terminal,
            redraw,
            false,
            BoundaryChars::new(),
        )
        .unwrap();
        if !nonempty {
            assert!(terminal.output.is_empty());
        }
        assert_eq!(terminal.output.contains(&b'>'), shown && nonempty);
        assert!(!terminal.output.windows(4).any(|bytes| bytes == b"\x1b[2J"));
        frame.complete();

        let mut idle = session.prepare_frame(frame_options());
        assert_eq!(idle.prompt().cursor().is_some(), shown && nonempty);
        assert!(!idle.changed());
        idle.complete();
        if !shown {
            session.events().set_active_components(Components::ALL);
            let mut restored = session.prepare_frame(frame_options());
            assert!(restored.prompt().cursor().is_none());
            restored.complete();
        }
    }
    assert_eq!(
        *resizes.borrow(),
        [
            Some(dimensions((8, 2))),
            None,
            Some(dimensions((8, 2))),
            Some(Dimensions::default()),
            Some(dimensions((8, 2))),
            Some(Dimensions::default()),
            Some(dimensions((8, 2))),
        ]
    );
}

#[test]
fn status_visibility_survives_resizing_in_both_orientations() {
    for reversed in [false, true] {
        let size = dimensions((20, 5));
        let mut layout = Layout::new(size, reversed, 0.4, Components::ALL);
        assert!(!layout.resolve(size, Components::ALL));
        let initial = (layout.prompt, layout.list, layout.status, layout.preview);
        let hidden = Components {
            status: false,
            ..Components::ALL
        };
        assert!(layout.resolve(size, hidden));
        assert!(layout.status.is_empty());
        assert_eq!(layout.list.height, 4);
        assert_eq!(layout.list.row, u16::from(reversed));
        assert_eq!((layout.prompt, layout.preview), (initial.0, initial.3));
        assert!(!layout.resolve(size, hidden));
        assert!(layout.resolve(dimensions((30, 1)), hidden));
        assert_eq!(layout.dimensions(), dimensions((30, 1)));
        assert!(layout.status.is_empty());
        assert!(layout.resolve(size, Components::ALL));
        assert_eq!(
            (layout.prompt, layout.list, layout.status, layout.preview),
            initial
        );
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
        let mut layout = Layout::new(size, false, ratio, Components::ALL);
        let initial = (layout.list, layout.preview);
        assert_eq!(layout.preview.width, expected);
        assert_eq!(layout.preview.column, width - expected);
        for small in [(0, 0), (5, 5), (101, 2)] {
            layout.resolve(dimensions(small), Components::ALL);
            assert!(layout.preview.is_empty());
            assert_eq!(layout.list.width, small.0);
        }
        layout.resolve(size, Components::ALL);
        assert_eq!((layout.list, layout.preview), initial);
    }
}

#[test]
fn divider_moves_from_displayed_width_and_survives_hiding_and_resize() {
    let size = dimensions((101, 5));
    let mut layout = Layout::new(size, false, 0.5, Components::ALL);
    for (columns, expected) in [
        (-1, 50),
        (1, 51),
        (i32::from(u16::MAX), 98),
        (-i32::from(u16::MAX), 3),
        (1, 4),
    ] {
        assert!(layout.shift_preview(columns, Components::ALL));
        assert_eq!(layout.preview.width, expected);
    }
    let hidden = Components {
        preview: false,
        ..Components::ALL
    };
    layout.resolve(size, hidden);
    assert!(layout.preview.is_empty());
    assert_eq!(layout.list.width, 101);
    assert!(!layout.shift_preview(20, hidden));
    assert!(layout.resolve(dimensions((202, 5)), hidden));
    assert!(layout.preview.is_empty());
    layout.resolve(dimensions((202, 5)), Components::ALL);
    assert_eq!(layout.preview.width, 8);
    assert!(layout.shift_preview(-1, Components::ALL));
    assert_eq!(layout.preview.width, 7);
    assert!(!layout.resolve(dimensions((202, 5)), Components::ALL));
    assert_eq!(layout.preview.width, 7);

    let mut layout = Layout::new(dimensions((20, 5)), false, 0.01, Components::ALL);
    assert!(!layout.shift_preview(-1, Components::ALL));
    layout.resolve(dimensions((1000, 5)), Components::ALL);
    assert_eq!(layout.preview.width, 10);
}

#[test]
fn undersized_previews_render_like_a_picker_without_preview() {
    for size in [(0, 4), (1, 1), (5, 4), (20, 2), (20, 0)] {
        assert_eq!(
            render(Components::ALL, true, size, EmptyPreview::default()),
            render(Components::ALL, true, size, Select)
        );
    }
    assert!(
        render(Components::ALL, true, (6, 3), EmptyPreview::default()).contains("\x1b[1;4H╭─╮")
    );
}

#[test]
fn preview_border_reads_content_after_resize() {
    struct ResizeError;

    impl Interface<&'static str> for ResizeError {
        type State = Pane;
        type Output<'items> = Option<&'items &'static str>;
        const PREVIEW: bool = true;

        fn init(&mut self, _: &Context<'_, &'static str>) -> Pane {
            let mut pane = Pane::new();
            pane.push_text("preview");
            pane
        }

        fn preview_resize(&mut self, pane: &mut Pane, size: Option<Dimensions>) {
            assert_eq!(size, Some(dimensions((13, 3))));
            pane.set_err(true);
        }

        fn preview_content<'a>(&'a self, pane: &'a Pane) -> Content<'a> {
            Content::Ready(pane)
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

    let mut driver = DriverOptions::new().driver(StrRenderer);
    let mut session = driver.session(ResizeError);
    let layout = Layout::new(dimensions((30, 5)), false, 0.5, session.active_components());
    let mut frame = session.prepare_frame(frame_options());
    let mut terminal = TestTerminal::default();
    super::draw(
        &layout,
        &mut frame,
        &mut terminal,
        Components::PREVIEW,
        false,
        BoundaryChars::new(),
    )
    .unwrap();
    frame.complete();
    let output = String::from_utf8(terminal.output).unwrap();
    assert!(output.contains(" Error "));
    assert!(output.contains("preview"));
}

#[test]
fn empty_and_tiny_targets_keep_preview_borders_in_bounds() {
    let chars = BoundaryChars {
        top_left: 'a',
        top_right: 'b',
        bottom_left: 'c',
        bottom_right: 'd',
        horizontal: '-',
        vertical: '|',
    };
    let mut pane = Pane::new();
    pane.push_text("Ａabc\nsecond");
    pane.set_err(true);
    for width in 0..=4 {
        for height in 0..=4 {
            let mut target = TestSurface::new(width, height);
            crate::preview::draw(&mut target, Content::Ready(&pane), Some(chars), '…').unwrap();
            assert_eq!(target.touched, width != 0 && height != 0);
            if width == 0 || height == 0 {
                continue;
            }
            let top = if width == 1 {
                "a".to_owned()
            } else {
                format!("a{}b", "-".repeat(usize::from(width - 2)))
            };
            assert_eq!(target.rows[0], top);
            if height > 1 {
                let bottom = if width == 1 {
                    "c".to_owned()
                } else {
                    format!("c{}d", "-".repeat(usize::from(width - 2)))
                };
                assert_eq!(target.rows[usize::from(height - 1)], bottom);
            }
        }
    }
}

#[test]
fn preview_resize_reaches_hidden_and_empty_targets() {
    let resizes = RefCell::new(Vec::new());
    let mut driver = DriverOptions::new().driver(StrRenderer);
    let mut session = driver.session(EmptyPreview(Some(&resizes)));
    let mut layout = Layout::new(dimensions((20, 5)), false, 0.5, session.active_components());
    for (size, enabled) in [
        ((20, 5), true),
        ((5, 4), true),
        ((20, 5), false),
        ((0, 4), true),
        ((20, 0), true),
        ((20, 5), true),
    ] {
        session.events().set_active_components(Components {
            preview: enabled,
            ..Components::ALL
        });
        layout.resolve(dimensions(size), session.active_components());
        let mut frame = session.prepare_frame(frame_options());
        if size.0 == 0 || size.1 == 0 {
            super::draw(
                &layout,
                &mut frame,
                &mut EmptyTerminal,
                Components::ALL,
                true,
                BoundaryChars::new(),
            )
            .unwrap();
        } else {
            super::draw(
                &layout,
                &mut frame,
                &mut TestTerminal::default(),
                Components::ALL,
                true,
                BoundaryChars::new(),
            )
            .unwrap();
        }
        frame.complete();
    }
    assert_eq!(
        *resizes.borrow(),
        [
            Some(dimensions((8, 3))),
            Some(Dimensions::default()),
            None,
            Some(Dimensions::default()),
            Some(dimensions((8, 3))),
        ]
    );
}
