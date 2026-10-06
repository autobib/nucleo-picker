use super::{Frame, LayoutEvent, Redraw};
use crate::{
    Picker, Terminal,
    component::{Component, NoPreview, PreviewComponent},
    match_engine::MatchEngine,
    match_list::MatchList,
    prompt::Prompt,
    render::StrRenderer,
    status_line::StatusLine,
};
use std::io::{self, Write};
#[derive(Default)]
pub(super) struct TestTerminal {
    pub(super) output: Vec<u8>,
    pub(super) size: (u16, u16),
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

    fn size(&mut self) -> io::Result<(u16, u16)> {
        Ok(self.size)
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

fn render(redraw: Redraw) -> String {
    render_at_size(redraw, (20, 4))
}
fn render_at_size(redraw: Redraw, size: (u16, u16)) -> String {
    render_with(redraw, size, NoPreview::<std::convert::Infallible>::new())
}
fn render_with<V>(redraw: Redraw, size: (u16, u16), mut preview: V) -> String
where
    V: PreviewComponent<String> + Component<MatchEngine<String, StrRenderer>>,
{
    let mut picker = Picker::<String, _>::new(StrRenderer);
    let mut prompt = Prompt::new(&mut picker.prompt, &picker.chars);
    let mut list = MatchList::new(&mut picker.list_state, &picker.chars, ());
    let mut status_line = StatusLine::new(&picker.chars);
    let mut frame = Frame::new(size, false, V::ENABLED.then_some(0.5));
    frame.resize(
        &picker.engine,
        &mut prompt,
        &mut list,
        &mut status_line,
        &mut preview,
    );
    let mut terminal = TestTerminal {
        size,
        ..TestTerminal::default()
    };
    frame
        .draw(
            &picker.engine,
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
fn match_list_redraw_does_not_draw_match_status() {
    let output = render(Redraw {
        prompt: false,
        list: true,
        status: false,
        preview: false,
    });

    assert!(!output.contains("0/0"));
}

#[test]
fn match_status_can_be_redrawn_independently() {
    let output = render(Redraw {
        prompt: false,
        list: false,
        status: true,
        preview: false,
    });

    assert!(output.contains("0/0"));
}

#[test]
fn prompt_and_list_redraw_preserve_unchanged_status() {
    let output = render(Redraw {
        prompt: true,
        list: true,
        ..Redraw::default()
    });
    assert!(!output.contains("\x1b[2J"));
    assert!(!output.contains("0/0"));
    assert!(output.contains('>'));
}

#[test]
fn full_redraw_uses_a_single_screen_clear_strategy() {
    let output = render(Redraw::full());

    assert!(output.contains("\x1b[2J"));
    assert!(!output.contains("\x1b[2K"));
}

#[test]
fn preview_only_redraw_does_not_draw_other_components() {
    let output = render(Redraw {
        prompt: false,
        list: false,
        status: false,
        preview: true,
    });

    assert_eq!(output, "\x1b[?2026h\x1b[4;3H\x1b[?2026l");
}

#[test]
fn full_screen_clear_does_not_require_a_preview_change() {
    let output = render(Redraw {
        prompt: true,
        list: true,
        status: true,
        preview: false,
    });

    assert!(output.contains("\x1b[2J"));
}

#[test]
fn zero_width_skips_preview_drawing() {
    let output = render_at_size(Redraw::full(), (0, 4));

    assert!(output.is_empty());
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

    for size in [(30, 8), (0, 8), (30, 0)] {
        let mut frame = Frame::new((20, 4), false, Some(0.5));
        let [mut prompt, mut list, mut status, mut preview] =
            std::array::from_fn(|_| Tracked::default());
        let mut terminal = TestTerminal {
            size,
            ..TestTerminal::default()
        };
        assert!(
            frame
                .render(
                    &(),
                    &mut prompt,
                    &mut list,
                    &mut status,
                    &mut preview,
                    &mut terminal,
                    Redraw {
                        prompt: true,
                        ..Redraw::default()
                    },
                )
                .unwrap()
        );
        let visible = size.0 != 0 && size.1 != 0;
        assert_eq!(terminal.output.is_empty(), !visible);
        if visible {
            assert!(terminal.output.windows(4).any(|bytes| bytes == b"\x1b[2J"));
        }
        terminal.output.clear();
        assert!(
            !frame
                .render(
                    &(),
                    &mut prompt,
                    &mut list,
                    &mut status,
                    &mut preview,
                    &mut terminal,
                    Redraw::default(),
                )
                .unwrap()
        );
        assert!(terminal.output.is_empty());
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

#[test]
fn size_changes_reassign_outer_rectangles() {
    let mut frame = Frame::new((20, 4), false, Some(0.5));
    assert_eq!(frame.list.width, 10);
    assert!(frame.update_size((30, 4)));
    assert_eq!(frame.list.width, 15);
    assert_eq!(frame.list.height, 2);
    assert!(frame.update_size((30, 8)));
    assert_eq!(frame.list.height, 6);
    assert!(!frame.update_size((30, 8)));
    assert!(frame.update_size((5, 8)));
    assert_eq!(frame.list.width, 5);
    assert!(frame.preview.is_empty());
}

#[test]
fn toggle_status_restores_layout_across_small_sizes_and_resizes() {
    for reversed in [false, true] {
        for ratio in [None, Some(0.4)] {
            for size in [(0, 0), (0, 5), (20, 1), (20, 2), (20, 5)] {
                let mut frame = Frame::new(size, reversed, ratio);
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

                frame.update_size((30, 8));
                assert!(frame.status.is_empty());
                assert_eq!(frame.list.height, 7);
                frame.update_size(size);
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
        size: frame.dimensions(),
        ..TestTerminal::default()
    };
    frame
        .render(
            &(),
            &mut NoPreview::<std::convert::Infallible>::new(),
            &mut NoPreview::<std::convert::Infallible>::new(),
            &mut NoPreview::<std::convert::Infallible>::new(),
            &mut NoPreview::<std::convert::Infallible>::new(),
            &mut terminal,
            Redraw::default(),
        )
        .unwrap()
}

#[test]
fn preview_ratios_round_and_reserve_space_for_both_panes() {
    for (width, ratio, expected) in [
        (101, 0.5, 51),
        (101, 0.4, 40),
        (101, 0.0, 3),
        (101, 1.0, 98),
        (6, 0.0, 3),
        (6, 1.0, 3),
        (u16::MAX, 0.5, 32768),
        (u16::MAX, 1.0, u16::MAX - 3),
    ] {
        for reversed in [false, true] {
            let frame = Frame::new((width, 5), reversed, Some(ratio));
            assert_eq!(frame.preview.width, expected);
            assert_eq!(frame.preview.column, width - expected);
            assert_eq!(frame.prompt.width, width - expected);
            assert_eq!(frame.status.width, width - expected);
            assert_eq!(frame.list.width, width - expected);
        }
    }
}

#[test]
fn resizing_preserves_the_ratio_through_rounding_clamping_and_hiding() {
    for ratio in [0.0, 0.1, 0.5, 0.9, 1.0] {
        let mut frame = Frame::new((101, 5), false, Some(ratio));
        let width = frame.preview.width;
        for size in [(20, 5), (6, 3), (5, 5), (101, 2), (0, 0), (101, 5)] {
            frame.update_size(size);
            assert_eq!(frame.preview_ratio, Some(ratio));
            if size.0 < 6 || size.1 < 3 {
                assert!(frame.preview.is_empty());
                assert_eq!(frame.list.width, size.0);
            }
        }
        assert_eq!(frame.preview.width, width);
    }
}

#[cfg(feature = "preview")]
mod preview {
    use super::*;
    use crate::preview::{
        Preview, PreviewConfig,
        pane::PreviewPane,
        request::{PreviewRequest, PreviewResponse},
    };

    #[test]
    fn toggle_preview_restores_the_adjusted_ratio_after_resizing() {
        for reversed in [false, true] {
            let mut frame = Frame::new((25, 5), reversed, Some(0.4));
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
            assert!(frame.update_size((50, 5)));
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
            let mut frame = Frame::new(size, false, Some(0.4));
            assert!(frame.preview_enabled());
            assert!(frame.preview.is_empty());
            frame.handle(LayoutEvent::TogglePreview);
            assert!(!frame.preview_enabled());
            frame.update_size((25, 5));
            assert!(frame.preview.is_empty());

            frame.update_size(size);
            frame.handle(LayoutEvent::TogglePreview);
            assert!(frame.preview_enabled());
            assert!(frame.preview.is_empty());
            frame.update_size((25, 5));
            assert_eq!(frame.preview.width, 10);
        }
    }

    #[test]
    fn adjustments_move_exactly_one_column_in_either_direction() {
        for width in [7, 13, 20, 101, u16::MAX] {
            let mut frame = Frame::new((width, 5), false, Some(0.5));
            let initial = frame.preview.width;
            assert!(!render_pending(&mut frame));
            for _ in 0..20 {
                frame.handle(LayoutEvent::MoveDividerRight(1));
                assert!(render_pending(&mut frame));
                assert_eq!(frame.preview.width, initial - 1);
                frame.handle(LayoutEvent::MoveDividerLeft(1));
                assert!(render_pending(&mut frame));
                assert_eq!(frame.preview.width, initial);
            }
            let ratio = frame.preview_ratio;
            frame.handle(LayoutEvent::MoveDividerLeft(0));
            assert!(!render_pending(&mut frame));
            assert_eq!(frame.preview_ratio, ratio);
        }
    }

    #[test]
    fn adjustments_start_from_the_clamped_width() {
        for (ratio, event, expected) in [
            (0.0, LayoutEvent::MoveDividerLeft(1), 4),
            (1.0, LayoutEvent::MoveDividerRight(1), 16),
        ] {
            let mut frame = Frame::new((20, 5), false, Some(ratio));
            frame.handle(event);
            assert!(render_pending(&mut frame));
            assert_eq!(frame.preview.width, expected);
            frame.update_size((100, 5));
            assert_eq!(frame.preview.width, expected * 5);
        }
    }

    #[test]
    fn adjustments_saturate_and_reverse_in_order_at_the_limits() {
        let mut frame = Frame::new((101, 5), false, Some(0.5));
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
            let mut frame = Frame::new(size, false, ratio);
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
        type AbortErr = std::convert::Infallible;
        fn preview(
            &mut self,
            _: &String,
            request: PreviewRequest<'_, String>,
            _: std::time::Duration,
        ) -> Result<PreviewResponse, Self::AbortErr> {
            Ok(PreviewResponse::Ready(request.ready()))
        }
    }
    fn render_preview(redraw: Redraw, size: (u16, u16)) -> String {
        render_with(
            redraw,
            size,
            PreviewPane::new(
                &PreviewConfig::default(),
                &crate::PickerChars::new(),
                EmptyPreview,
            ),
        )
    }
    #[test]
    fn partial_redraws_preserve_the_preview_without_repainting_it() {
        for redraw in [
            Redraw {
                prompt: true,
                list: false,
                status: false,
                preview: false,
            },
            Redraw {
                prompt: false,
                list: false,
                status: true,
                preview: false,
            },
            Redraw {
                prompt: false,
                list: true,
                status: false,
                preview: false,
            },
        ] {
            let output = render_preview(redraw, (20, 4));
            assert!(!output.contains("\x1b[2J"));
            assert!(!output.contains("\x1b[2K"));
            assert!(!output.contains("\x1b[K"));
            assert!(!output.contains('╭'));
        }
    }

    #[test]
    fn screen_clears_restore_the_preview_after_the_picker() {
        let output = render_preview(
            Redraw {
                prompt: true,
                list: true,
                status: true,
                preview: false,
            },
            (20, 4),
        );
        assert!(output.contains("\x1b[2J"));
        assert!(output.find('>').unwrap() < output.find('╭').unwrap());
        assert!(output.contains("╭────────╮"));
        assert!(output.contains("╰────────╯"));
        assert!(output.ends_with("\x1b[4;3H\x1b[?2026l"));
    }

    #[test]
    fn preview_redraw_clears_only_its_own_interior() {
        let output = render_preview(
            Redraw {
                prompt: false,
                list: false,
                status: false,
                preview: true,
            },
            (20, 4),
        );
        assert!(!output.contains('>'));
        assert!(!output.contains("0/0"));
        assert!(output.contains("\x1b[2;11H\x1b[11G          \x1b[11G│        │"));
        assert!(output.contains("\x1b[3;11H\x1b[11G          \x1b[11G│        │"));
    }

    #[test]
    fn preview_falls_back_to_full_width_when_its_interior_cannot_fit() {
        for size in [(0, 4), (1, 1), (5, 4), (20, 2), (20, 0)] {
            assert_eq!(
                Frame::new(size, false, Some(0.5))
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
            Frame::new((6, 3), false, Some(0.5))
                .preview
                .height
                .saturating_sub(2),
            1
        );
        assert!(render_preview(Redraw::full(), (6, 3)).contains("\x1b[1;4H\x1b[4G╭─╮"));
    }
}
