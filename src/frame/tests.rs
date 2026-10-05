use super::{Frame, Redraw};
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
    let frame = Frame::new(size, false, V::ENABLED);
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
fn size_changes_reassign_outer_rectangles() {
    let mut frame = Frame::new((20, 4), false, true);
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

#[cfg(feature = "preview")]
mod preview {
    use super::*;
    use crate::preview::{
        Preview, PreviewConfig,
        pane::PreviewPane,
        request::{PreviewRequest, PreviewResponse},
    };
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
                Frame::new(size, false, true)
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
            Frame::new((6, 3), false, true)
                .preview
                .height
                .saturating_sub(2),
            1
        );
        assert!(render_preview(Redraw::full(), (6, 3)).contains("\x1b[1;4H\x1b[4G╭─╮"));
    }
}
