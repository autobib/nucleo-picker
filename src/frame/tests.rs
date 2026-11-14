use super::{Frame, LayoutEvent, Redraw};
use crate::{
    Picker, Terminal,
    component::{Component, NoPreview, PreviewComponent},
    match_list::MatchList,
    prompt::Prompt,
    render::StrRenderer,
    session::Dimensions,
    status_line::StatusLine,
};
use std::io::{self, Write};
#[derive(Default)]
pub(super) struct TestTerminal {
    pub(super) output: Vec<u8>,
    pub(super) dimensions: Dimensions,
    pub(super) rendering: bool,
}

impl Write for TestTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for TestTerminal {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        Ok(self.dimensions)
    }

    fn begin_render(&mut self) -> io::Result<()> {
        assert!(!self.rendering);
        self.rendering = true;
        Ok(())
    }

    fn end_render(&mut self) -> io::Result<()> {
        assert!(self.rendering);
        self.rendering = false;
        Ok(())
    }
}

fn dimensions((width, height): (u16, u16)) -> Dimensions {
    Dimensions { width, height }
}

fn render_at_size(redraw: Redraw, size: (u16, u16)) -> String {
    render_with(redraw, size, NoPreview)
}
fn render_with<V>(redraw: Redraw, size: (u16, u16), mut preview: V) -> String
where
    V: PreviewComponent<String>,
{
    let mut picker = Picker::<String, _>::new(StrRenderer);
    let mut prompt = Prompt::new(&mut picker.core.prompt, &picker.core.chars);
    let mut list = MatchList::new(&mut picker.core.list_state, &picker.core.chars, ());
    let mut status_line = StatusLine::new(&picker.core.chars);
    let mut frame = Frame::new(dimensions(size), false, V::ENABLED.then_some(0.5));
    frame.resize(
        &picker.core.engine,
        &mut prompt,
        &mut list,
        &mut status_line,
        &mut preview,
    );
    let mut terminal = TestTerminal {
        dimensions: dimensions(size),
        ..TestTerminal::default()
    };
    frame
        .draw(
            &picker.core.engine,
            &mut prompt,
            &mut list,
            &mut status_line,
            &mut preview,
            &mut terminal,
            redraw,
        )
        .unwrap();
    assert!(!terminal.rendering);
    String::from_utf8(terminal.output).unwrap()
}
#[test]
fn redraws_respect_component_boundaries_and_restore_the_cursor() {
    for (render, preview_enabled) in [
        (render_at_size as fn(Redraw, (u16, u16)) -> String, false),
        #[cfg(feature = "preview")]
        (preview::render_preview, true),
    ] {
        for (flags, screen_clear) in [
            ([false, false, false, false], false),
            ([true, false, false, false], false),
            ([false, true, false, false], false),
            ([false, false, true, false], false),
            ([false, false, false, true], false),
            ([true, true, false, false], false),
            ([true, true, true, false], true),
            ([true, true, true, true], true),
        ] {
            let [prompt, list, status, preview] = flags;
            let output = render(
                Redraw {
                    prompt,
                    list,
                    status,
                    preview,
                },
                (20, 4),
            );
            let draw_preview = preview_enabled && (preview || screen_clear);
            assert_eq!(
                [
                    output.contains('>'),
                    output.contains("\x1b[1;1H"),
                    output.contains("0/0"),
                    output.contains('╭')
                ],
                [prompt, list, status, draw_preview],
                "flags: {flags:?}, preview: {preview_enabled}",
            );
            assert_eq!(output.matches("\x1b[2J").count(), usize::from(screen_clear));
            assert!(!output.contains("\x1b[2K"));
            assert_eq!(
                output.contains("\x1b[K"),
                !preview_enabled && !screen_clear && (prompt || list)
            );
            assert!(output.starts_with("\x1b[?2026h"));
            assert!(output.ends_with("\x1b[4;3H\x1b[?2026l"));
            if draw_preview {
                assert!(output.contains("╭────────╮"));
                assert!(output.contains("╰────────╯"));
                if screen_clear {
                    assert!(output.find('>').unwrap() < output.find('╭').unwrap());
                } else {
                    assert!(output.contains("\x1b[2;11H\x1b[11G          \x1b[11G│        │"));
                    assert!(output.contains("\x1b[3;11H\x1b[11G          \x1b[11G│        │"));
                }
            } else if !prompt && !list && !status {
                assert_eq!(output, "\x1b[?2026h\x1b[4;3H\x1b[?2026l");
            }
        }
    }
}

#[test]
fn empty_areas_skip_component_drawing() {
    use crate::rect::{Area, ClearState, Rect};

    struct Hidden;
    impl Component<()> for Hidden {
        fn resize(&mut self, _area: Area, _engine: &()) {}

        fn draw<D: Rect>(&mut self, _engine: &(), _rect: &mut D) -> io::Result<()> {
            panic!("empty areas must not be drawn");
        }
    }

    for (width, height) in [(0, 2), (8, 0)] {
        for clear in [
            ClearState::Precleared,
            ClearState::ToEndOfLine,
            ClearState::WithinRect,
        ] {
            let mut output = Vec::new();
            super::draw_component(
                &mut Hidden,
                &(),
                &mut output,
                Area {
                    width,
                    height,
                    ..Area::default()
                },
                clear,
            )
            .unwrap();
            assert!(output.is_empty());
        }
    }
}

#[test]
fn render_applies_terminal_resize_to_all_components_before_drawing() {
    use crate::rect::{Area, Rect};

    #[derive(Default)]
    struct Tracked {
        area: Area,
        resizes: usize,
        draws: usize,
    }
    impl Component<()> for Tracked {
        fn resize(&mut self, area: Area, _engine: &()) {
            self.area = area;
            self.resizes += 1;
        }

        fn draw<D: Rect>(&mut self, _engine: &(), rect: &mut D) -> io::Result<()> {
            assert_eq!(self.area.width, rect.width().get());
            assert_eq!(self.area.height, rect.height().get());
            self.draws += 1;
            Ok(())
        }
    }

    for (size, split_width, list_height) in [
        ((30, 4), 15, 2),
        ((30, 8), 15, 6),
        ((5, 8), 5, 6),
        ((0, 8), 0, 6),
        ((30, 0), 30, 0),
    ] {
        for ratio in [None, Some(0.5)] {
            let mut frame = Frame::new(dimensions((20, 4)), false, ratio);
            assert_eq!(frame.list.width, if ratio.is_some() { 10 } else { 20 });
            let [mut prompt, mut list, mut status, mut preview] =
                std::array::from_fn(|_| Tracked::default());
            let mut terminal = TestTerminal {
                dimensions: dimensions(size),
                ..TestTerminal::default()
            };
            for redraw in [true, false] {
                assert_eq!(
                    frame
                        .render(
                            &(),
                            &mut prompt,
                            &mut list,
                            &mut status,
                            &mut preview,
                            &mut terminal,
                            Redraw {
                                prompt: redraw,
                                ..Redraw::default()
                            },
                        )
                        .unwrap(),
                    redraw,
                );
                let visible = redraw && size.0 != 0 && size.1 != 0;
                assert_eq!(terminal.output.is_empty(), !visible);
                assert!(!terminal.rendering);
                if visible {
                    assert!(terminal.output.windows(4).any(|bytes| bytes == b"\x1b[2J"));
                }
                terminal.output.clear();
            }
            assert_eq!(frame.dimensions(), dimensions(size));
            let list_width = if ratio.is_some() { split_width } else { size.0 };
            assert_eq!(
                (frame.list.width, frame.list.height),
                (list_width, list_height)
            );
            assert_eq!(frame.preview.is_empty(), list_width == size.0);
            assert!(!frame.update_dimensions(dimensions(size)));
            for (component, area) in [
                (prompt, frame.prompt),
                (list, frame.list),
                (status, frame.status),
                (preview, frame.preview),
            ] {
                assert_eq!(component.area, area);
                assert_eq!(component.resizes, 1);
                assert_eq!(component.draws, usize::from(!area.is_empty()));
            }
        }
    }
}

#[test]
fn toggle_status_restores_layout_across_small_sizes_and_resizes() {
    for reversed in [false, true] {
        for ratio in [None, Some(0.4)] {
            for size in [(0, 0), (0, 5), (20, 1), (20, 2), (20, 5)] {
                let mut frame = Frame::new(dimensions(size), reversed, ratio);
                let initial = [frame.prompt, frame.list, frame.status, frame.preview];
                frame.handle(LayoutEvent::ToggleStatus);
                assert!(!frame.status_enabled());
                assert!(frame.status.is_empty());
                assert_eq!(frame.prompt, initial[0]);
                assert_eq!(frame.preview, initial[3]);
                assert_eq!(frame.list.width, initial[1].width);
                assert_eq!(frame.list.height, size.1.saturating_sub(1));
                assert_eq!(frame.list.row, if reversed { size.1.min(1) } else { 0 });
                assert!(render_pending(&mut frame));
                assert!(!render_pending(&mut frame));

                frame.update_dimensions(dimensions((30, 8)));
                assert!(frame.status.is_empty());
                assert_eq!(frame.list.height, 7);
                frame.update_dimensions(dimensions(size));
                frame.handle(LayoutEvent::ToggleStatus);
                assert!(frame.status_enabled());
                assert_eq!(
                    [frame.prompt, frame.list, frame.status, frame.preview],
                    initial,
                );
                assert!(render_pending(&mut frame));
                assert!(!render_pending(&mut frame));
            }
        }
    }
}

fn render_pending(frame: &mut Frame) -> bool {
    let mut terminal = TestTerminal {
        dimensions: frame.dimensions(),
        ..TestTerminal::default()
    };
    frame
        .render(
            &(),
            &mut NoPreview,
            &mut NoPreview,
            &mut NoPreview,
            &mut NoPreview,
            &mut terminal,
            Redraw::default(),
        )
        .unwrap()
}

#[test]
fn preview_ratios_round_clamp_and_survive_resizing() {
    for (width, ratio, expected) in [
        (101, 0.5, 51),
        (101, 0.4, 40),
        (101, 0.0, 3),
        (101, 0.1, 10),
        (101, 0.9, 91),
        (101, 1.0, 98),
        (6, 0.0, 3),
        (6, 1.0, 3),
        (u16::MAX, 0.5, 32768),
        (u16::MAX, 1.0, u16::MAX - 3),
    ] {
        for reversed in [false, true] {
            let mut frame = Frame::new(dimensions((width, 5)), reversed, Some(ratio));
            assert_eq!(frame.preview.width, expected);
            assert_eq!(frame.preview.column, width - expected);
            assert_eq!(frame.prompt.width, width - expected);
            assert_eq!(frame.status.width, width - expected);
            assert_eq!(frame.list.width, width - expected);
            for size in [(20, 5), (6, 3), (5, 5), (101, 2), (0, 0), (width, 5)] {
                frame.update_dimensions(dimensions(size));
                assert_eq!(frame.preview_ratio, Some(ratio));
                if size.0 < 6 || size.1 < 3 {
                    assert!(frame.preview.is_empty());
                    assert_eq!(frame.list.width, size.0);
                }
            }
            assert_eq!(frame.preview.width, expected);
        }
    }
}

#[cfg(feature = "preview")]
mod preview {
    use super::*;
    use crate::preview::{
        DEFAULT_CACHE_SIZE, Preview, PreviewConfig,
        controller::PreviewController,
        request::{Request, Response},
    };

    #[test]
    fn toggle_preview_restores_the_adjusted_ratio_after_resizing() {
        for reversed in [false, true] {
            let mut frame = Frame::new(dimensions((25, 5)), reversed, Some(0.4));
            frame.handle(LayoutEvent::MoveDividerLeft(1));
            assert!(render_pending(&mut frame));
            let ratio = frame.preview_ratio;
            assert_eq!(frame.preview.width, 11);

            frame.handle(LayoutEvent::TogglePreview);
            assert!(!frame.preview_enabled());
            assert!(frame.preview.is_empty());
            assert_eq!(frame.list.width, 25);
            assert_eq!(frame.prompt.width, 25);
            assert_eq!(frame.status.width, 25);
            assert!(render_pending(&mut frame));
            assert!(!render_pending(&mut frame));

            frame.handle(LayoutEvent::MoveDividerLeft(1));
            frame.handle(LayoutEvent::MoveDividerRight(1));
            assert!(!render_pending(&mut frame));
            assert_eq!(frame.preview_ratio, ratio);
            assert!(frame.update_dimensions(dimensions((50, 5))));
            assert!(frame.preview.is_empty());
            assert_eq!(frame.list.width, 50);

            frame.handle(LayoutEvent::TogglePreview);
            assert!(frame.preview_enabled());
            assert_eq!(frame.preview_ratio, ratio);
            assert_eq!(frame.preview.width, 22);
            assert!(render_pending(&mut frame));
            assert!(!render_pending(&mut frame));
        }
    }

    #[test]
    fn toggle_preview_remembers_visibility_when_the_terminal_is_too_small() {
        for size in [(0, 0), (5, 5), (25, 2)] {
            let mut frame = Frame::new(dimensions(size), false, Some(0.4));
            assert!(frame.preview_enabled());
            assert!(frame.preview.is_empty());
            frame.handle(LayoutEvent::TogglePreview);
            assert!(!frame.preview_enabled());
            frame.update_dimensions(dimensions((25, 5)));
            assert!(frame.preview.is_empty());

            frame.update_dimensions(dimensions(size));
            frame.handle(LayoutEvent::TogglePreview);
            assert!(frame.preview_enabled());
            assert!(frame.preview.is_empty());
            frame.update_dimensions(dimensions((25, 5)));
            assert_eq!(frame.preview.width, 10);
        }
    }

    #[test]
    fn adjustments_use_the_displayed_width_and_preserve_it_through_resize() {
        let left = LayoutEvent::MoveDividerLeft as fn(u16) -> LayoutEvent;
        let right = LayoutEvent::MoveDividerRight as fn(u16) -> LayoutEvent;
        for (width, ratio, forward, backward, expected) in [
            (7, 0.5, right, left, 3),
            (13, 0.5, right, left, 6),
            (20, 0.5, right, left, 9),
            (101, 0.5, right, left, 50),
            (u16::MAX, 0.5, right, left, 32767),
            (20, 0.0, left, right, 4),
            (20, 1.0, right, left, 16),
        ] {
            let mut frame = Frame::new(dimensions((width, 5)), false, Some(ratio));
            let initial = frame.preview.width;
            assert!(!render_pending(&mut frame));
            for _ in 0..20 {
                frame.handle(forward(1));
                assert!(render_pending(&mut frame));
                assert_eq!(frame.preview.width, expected);
                frame.handle(backward(1));
                assert!(render_pending(&mut frame));
                assert_eq!(frame.preview.width, initial);
            }
            let ratio = frame.preview_ratio;
            frame.handle(forward(0));
            frame.handle(backward(0));
            assert!(!render_pending(&mut frame));
            assert_eq!(frame.preview_ratio, ratio);
            frame.handle(forward(1));
            if width == 20 {
                frame.update_dimensions(dimensions((100, 5)));
                assert_eq!(frame.preview.width, expected * 5);
            }
            frame.update_dimensions(dimensions((5, 5)));
            assert!(frame.preview.is_empty());
            frame.update_dimensions(dimensions((width, 5)));
            assert_eq!(frame.preview.width, expected);
        }
    }

    #[test]
    fn adjustments_saturate_and_reverse_in_order_at_the_limits() {
        let mut frame = Frame::new(dimensions((101, 5)), false, Some(0.5));
        frame.handle(LayoutEvent::MoveDividerLeft(u16::MAX));
        frame.handle(LayoutEvent::MoveDividerLeft(1));
        assert_eq!(frame.preview.width, 98);
        assert!(render_pending(&mut frame));
        assert!(!render_pending(&mut frame));

        frame.handle(LayoutEvent::MoveDividerRight(u16::MAX));
        frame.handle(LayoutEvent::MoveDividerLeft(1));
        frame.handle(LayoutEvent::MoveDividerRight(0));
        assert_eq!(frame.preview.width, 4);
        assert!(render_pending(&mut frame));
        assert!(!render_pending(&mut frame));

        frame.handle(LayoutEvent::MoveDividerLeft(u16::MAX));
        frame.handle(LayoutEvent::MoveDividerRight(1));
        assert_eq!(frame.preview.width, 97);
        assert!(render_pending(&mut frame));
        assert!(!render_pending(&mut frame));
    }

    #[test]
    fn ineffective_adjustments_preserve_the_exact_ratio() {
        for (size, ratio, shift) in [
            ((101, 5), Some(0.5), 0),
            ((20, 5), Some(0.01), -1),
            ((20, 5), Some(0.99), 1),
            ((6, 3), Some(0.4), -1),
            ((6, 3), Some(0.4), 1),
            ((5, 5), Some(0.4), 1),
            ((101, 2), Some(0.4), -1),
            ((0, 0), Some(0.4), 1),
            ((101, 5), None, 1),
        ] {
            let mut frame = Frame::new(dimensions(size), false, ratio);
            frame.handle(if shift >= 0 {
                LayoutEvent::MoveDividerLeft(shift as u16)
            } else {
                LayoutEvent::MoveDividerRight((-shift) as u16)
            });
            assert!(!render_pending(&mut frame));
            assert_eq!(frame.preview_ratio, ratio);
        }
    }

    struct EmptyPreview;
    impl Preview<String> for EmptyPreview {
        fn preview(&mut self, request: Request<'_, String>, _: std::time::Duration) -> Response {
            Response::Ready(request.ready())
        }
    }
    pub(super) fn render_preview(redraw: Redraw, size: (u16, u16)) -> String {
        render_with(
            redraw,
            size,
            PreviewController::new(
                &PreviewConfig::default(),
                &crate::PickerChars::new(),
                DEFAULT_CACHE_SIZE,
                EmptyPreview,
            ),
        )
    }
    #[test]
    fn preview_falls_back_to_full_width_when_its_interior_cannot_fit() {
        for size in [(0, 4), (1, 1), (5, 4), (20, 2), (20, 0)] {
            assert_eq!(
                Frame::new(dimensions(size), false, Some(0.5))
                    .preview
                    .height
                    .saturating_sub(2),
                0
            );
            assert_eq!(
                render_preview(Redraw::full(), size),
                render_at_size(Redraw::full(), size)
            );
        }
        assert_eq!(
            Frame::new(dimensions((6, 3)), false, Some(0.5))
                .preview
                .height
                .saturating_sub(2),
            1
        );
        assert!(render_preview(Redraw::full(), (6, 3)).contains("\x1b[1;4H\x1b[4G╭─╮"));
    }
}
