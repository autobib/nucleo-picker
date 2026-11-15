#![cfg(all(feature = "preview", feature = "unstable-backend"))]

use std::{
    cell::Cell,
    io,
    num::NonZero,
    time::{Duration, Instant},
};

use nucleo_picker::{
    Interface, PickerOptions, Terminal,
    event::{Event, EventSource, RecvError},
    preview::{Content, Pane, PreviewEvent},
    render::StrRenderer,
    session::{Context, Dimensions, Exit, ItemId, Redraw},
};

#[derive(Default)]
struct TestTerminal {
    buffer: Vec<u8>,
    renders: Vec<String>,
    changed: Vec<bool>,
}

impl io::Write for TestTerminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
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
        Ok(Dimensions {
            width: 60,
            height: 8,
        })
    }

    fn end_render(&mut self) -> io::Result<()> {
        self.renders
            .push(String::from_utf8(std::mem::take(&mut self.buffer)).unwrap());
        Ok(())
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.changed.push(changed);
        Ok(())
    }
}

struct Script<I>(I);

impl<I: Iterator<Item = Result<Event<&'static str>, RecvError>>> EventSource for Script<I> {
    type AbortErr = &'static str;

    fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
        Err(RecvError::Timeout)
    }

    fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
        self.0.next().expect("event script exhausted")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Initial,
    AfterInitial,
    Before,
    After,
    Event,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Queue,
    Preview,
    Both,
}

struct Custom<'a> {
    phase: Phase,
    change: Change,
    ended: &'a Cell<bool>,
    content_reads: &'a Cell<usize>,
}

struct State<'a> {
    ended: &'a Cell<bool>,
    queued: ItemId,
    limit: NonZero<u32>,
    pane: Pane,
    frame: usize,
}

struct Output<'items> {
    cx: Context<'items, String>,
    selected: Option<ItemId>,
}

impl<'items> IntoIterator for Output<'items> {
    type Item = &'items String;
    type IntoIter = std::option::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.selected
            .map(|id| self.cx.matches().item(id).unwrap().value())
            .into_iter()
    }
}

impl Custom<'_> {
    fn change(&mut self, state: &mut State<'_>, cx: &Context<'_, String>) -> Redraw {
        let mut redraw = Redraw::NONE;
        if matches!(self.change, Change::Queue | Change::Both) {
            state.queued = cx.matches().get(1).unwrap().id();
            state.limit = NonZero::new(2).unwrap();
            redraw |= Redraw::QUEUE;
        }
        if matches!(self.change, Change::Preview | Change::Both) {
            state.pane.clear();
            state.pane.push_text("new preview");
            redraw |= Redraw::PREVIEW;
        }
        redraw
    }
}

impl<'a> Interface<String> for Custom<'a> {
    type State = State<'a>;
    type Output<'items> = Output<'items>;

    const HAS_PREVIEW: bool = true;

    fn init(&mut self, cx: &Context<'_, String>) -> Self::State {
        assert!(cx.hovered().is_none());
        assert!(cx.preview_enabled());
        let mut pane = Pane::new();
        pane.push_text("old preview");
        State {
            ended: self.ended,
            queued: cx.matches().get(0).unwrap().id(),
            limit: NonZero::new(1).unwrap(),
            pane,
            frame: 0,
        }
    }

    fn is_queued(&self, state: &Self::State, id: ItemId) -> bool {
        state.queued == id
    }

    fn queued_count(&self, _: &Self::State) -> Option<usize> {
        Some(1)
    }

    fn queue_limit(&self, state: &Self::State) -> Option<NonZero<u32>> {
        Some(state.limit)
    }

    fn before_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, String>,
        _: bool,
        _: Instant,
    ) -> Redraw {
        if matches!(
            (self.phase, state.frame),
            (Phase::Initial, 0) | (Phase::Before, 1)
        ) {
            self.change(state, cx)
        } else {
            Redraw::NONE
        }
    }

    fn after_frame(
        &mut self,
        state: &mut Self::State,
        cx: &Context<'_, String>,
        _: Instant,
    ) -> Redraw {
        let redraw = if matches!(
            (self.phase, state.frame),
            (Phase::AfterInitial, 0) | (Phase::After, 1)
        ) {
            self.change(state, cx)
        } else {
            Redraw::NONE
        };
        state.frame += 1;
        redraw
    }

    fn preview_event(
        &mut self,
        state: &mut Self::State,
        _: PreviewEvent,
        cx: &Context<'_, String>,
    ) -> Redraw {
        if self.phase == Phase::Event {
            self.change(state, cx)
        } else {
            Redraw::NONE
        }
    }

    fn preview_content<'s>(&'s self, state: &'s Self::State) -> Content<'s> {
        self.content_reads.set(self.content_reads.get() + 1);
        Content::Ready(&state.pane)
    }

    fn on_end(&mut self, state: &mut Self::State) {
        assert!(!state.ended.replace(true));
    }

    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        assert!(state.ended.get());
        assert_eq!(state.frame, 4);
        Output {
            cx,
            selected: matches!(exit, Exit::Selected(_)).then_some(state.queued),
        }
    }
}

#[test]
fn public_hooks_redraw_selectively_and_output_outlives_the_interface() {
    for phase in [
        Phase::Initial,
        Phase::AfterInitial,
        Phase::Before,
        Phase::After,
        Phase::Event,
    ] {
        for change in [Change::Queue, Change::Preview, Change::Both] {
            let queue = matches!(change, Change::Queue | Change::Both);
            let preview = matches!(change, Change::Preview | Change::Both);
            let mut picker = PickerOptions::new()
                .sort_results(false)
                .frame_interval(Duration::from_secs(5))
                .picker(StrRenderer);
            picker.push_batch(["alpha", "beta"].map(str::to_owned));
            picker.filter().unwrap();
            let expected =
                picker.matches().get(usize::from(queue)).unwrap().value() as *const String;
            let mut terminal = TestTerminal::default();
            let output = {
                let ended = Cell::new(false);
                let content_reads = Cell::new(0);
                let output = picker
                    .as_interface(Custom {
                        phase,
                        change,
                        ended: &ended,
                        content_reads: &content_reads,
                    })
                    .pick_with_terminal_io(
                        Script(
                            [
                                Ok(Event::Preview(PreviewEvent::Refresh)),
                                Err(RecvError::Timeout),
                                Err(RecvError::Timeout),
                                Err(RecvError::Timeout),
                                Ok(Event::Select),
                            ]
                            .into_iter(),
                        ),
                        &mut terminal,
                    )
                    .unwrap();
                assert!(ended.get());
                let preview_repaints = 1 + usize::from(phase != Phase::Initial && preview);
                assert_eq!(content_reads.get(), preview_repaints);
                output
            };
            assert_eq!(output.cx.hovered().unwrap().value(), "alpha");
            assert_eq!(
                output.into_iter().next().unwrap() as *const String,
                expected
            );
            assert_eq!(
                terminal.changed,
                [
                    matches!(phase, Phase::AfterInitial | Phase::Before | Phase::Event),
                    phase == Phase::After,
                    false
                ],
                "{phase:?}, {change:?}",
            );
            assert_eq!(
                terminal.renders.len(),
                1 + usize::from(phase != Phase::Initial)
            );
            let rendered = terminal.renders.last().unwrap();
            if queue {
                assert!(rendered.contains("(1/2)"));
                assert_eq!(rendered.matches('┃').count(), 1);
            }
            if preview {
                assert!(rendered.contains("new preview"));
            }
            if phase != Phase::Initial {
                assert!(!rendered.contains('>'));
                assert_eq!(rendered.contains("alpha"), queue);
                assert_eq!(rendered.contains("beta"), queue);
                assert_eq!(rendered.contains("2/2"), queue);
                assert_eq!(rendered.contains('╭'), preview);
            }
        }
    }
}
