use std::{
    cell::{Cell, RefCell},
    io,
    time::Duration,
};

use super::{FrameEvents, frame_events};
use crate::{
    Picker, PickerOptions, Terminal,
    error::PickError,
    event::{Event, PreviewEvent, RecvError},
    preview::{
        Buffer, Preview,
        request::{QueuedRequest, Request, Response},
    },
    render::StrRenderer,
    session::{Dimensions, ItemRef},
};

#[derive(Default)]
struct TestPreviewer {
    hovered: Vec<Option<&'static str>>,
    cancelled_on_clear: Vec<bool>,
    requested: Vec<&'static str>,
    queued: Vec<QueuedRequest<&'static str>>,
}

impl Preview<&'static str> for TestPreviewer {
    fn preview(&mut self, request: Request<'_, &'static str>, _: Duration) -> Response {
        assert_eq!(self.hovered.last(), Some(&Some(*request.item())));
        self.requested.push(request.item());
        let (pending, queued) = request.defer();
        self.queued.push(queued);
        Response::Pending(pending)
    }

    fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
        if item.is_none() {
            self.cancelled_on_clear
                .push(self.queued.iter().all(QueuedRequest::is_cancelled));
        }
        self.hovered.push(item.map(|item| *item.value()));
    }
}

struct SharedPreviewer<'a>(&'a RefCell<TestPreviewer>);

impl Preview<&'static str> for SharedPreviewer<'_> {
    fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
        self.0.borrow_mut().preview(request, timeout)
    }

    fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
        self.0.borrow_mut().hover_changed(item);
    }
}

fn picker(items: impl IntoIterator<Item = &'static str>) -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    picker.filter().unwrap();
    picker
}

#[derive(Default)]
struct RunnerTerminal<'a> {
    renders: Option<&'a Cell<usize>>,
    cleanups: usize,
    fail_write: bool,
    fail_cleanup: bool,
}

impl io::Write for RunnerTerminal<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_write {
            Err(io::Error::other("write failed"))
        } else {
            Ok(bytes.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for RunnerTerminal<'_> {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.cleanups += 1;
        if self.fail_cleanup {
            Err(io::Error::other("cleanup failed"))
        } else {
            Ok(())
        }
    }

    fn dimensions(&mut self) -> io::Result<Dimensions> {
        Ok(Dimensions {
            width: 20,
            height: 10,
        })
    }

    fn end_render(&mut self) -> io::Result<()> {
        if let Some(renders) = self.renders {
            renders.set(renders.get() + 1);
        }
        Ok(())
    }
}

#[test]
fn prefetch_follows_rendering_and_progresses_on_idle_frames() {
    struct Previewer<'a> {
        renders: &'a Cell<usize>,
        queued: &'a RefCell<Vec<QueuedRequest<&'static str>>>,
    }

    impl Preview<&'static str> for Previewer<'_> {
        fn prefetch_limit(&self) -> usize {
            3
        }

        fn preview(&mut self, request: Request<'_, &'static str>, _: Duration) -> Response {
            let item = request.item();
            let mut pane = request.ready();
            pane.push_text(item);
            Response::Ready(pane)
        }

        fn prefetch(
            &mut self,
            request: Request<'_, &'static str>,
            _: Duration,
        ) -> Result<Response, Buffer> {
            assert!(self.renders.get() > 0, "prefetch preceded presentation");
            let (pending, queued) = request.defer();
            self.queued.borrow_mut().push(queued);
            Ok(Response::Pending(pending))
        }
    }

    let renders = Cell::new(0);
    let queued = RefCell::new(Vec::new());
    let mut previewer = Previewer {
        renders: &renders,
        queued: &queued,
    };
    let mut terminal = RunnerTerminal {
        renders: Some(&renders),
        ..RunnerTerminal::default()
    };
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    picker.push_batch(["a", "b", "c", "d"]);
    picker.filter().unwrap();
    let mut active = None;
    picker
        .with_preview(&mut previewer)
        .pick_with_terminal_io(
            FrameEvents(|| {
                assert_eq!(renders.get(), 1);
                if active.is_none() {
                    active = Some(queued.borrow_mut().pop().unwrap().start().unwrap());
                    assert!(queued.borrow().is_empty());
                    Err(RecvError::Timeout)
                } else {
                    assert!(!queued.borrow().is_empty(), "idle frame did not prefetch");
                    Ok(Event::Quit)
                }
            }),
            &mut terminal,
        )
        .unwrap();
}

#[test]
fn refresh_then_quit_cancels_work_without_generating_a_replacement() {
    let previewer = RefCell::new(TestPreviewer::default());
    let mut terminal = RunnerTerminal::default();
    let mut picker = PickerOptions::new()
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer)
        .with_preview(SharedPreviewer(&previewer));
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    let mut events = [Event::Preview(PreviewEvent::Refresh), Event::Quit].into_iter();
    picker
        .pick_with_terminal_io(
            FrameEvents(|| {
                let event = events.next().unwrap();
                if matches!(event, Event::Quit) {
                    let previewer = previewer.borrow();
                    assert_eq!(previewer.requested, ["alpha"]);
                    assert!(previewer.queued[0].is_cancelled());
                }
                Ok(event)
            }),
            &mut terminal,
        )
        .unwrap();
    let previewer = previewer.borrow();
    assert_eq!(previewer.requested, ["alpha"]);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.hovered, [Some("alpha"), None]);
    assert_eq!(terminal.cleanups, 1);
}

#[test]
fn presentation_and_cleanup_errors_cancel_pending_previews() {
    for fail_write in [false, true] {
        let previewer = TestPreviewer::default();
        let mut terminal = RunnerTerminal {
            fail_write,
            fail_cleanup: !fail_write,
            ..RunnerTerminal::default()
        };
        let mut picker = picker(["alpha"]).with_preview(previewer);
        let result = picker.pick_with_terminal_io(frame_events([Ok(Event::Quit)]), &mut terminal);
        assert!(matches!(result, Err(PickError::IO(_))));
        let previewer = picker.previewer();
        assert_eq!(terminal.cleanups, 1);
        assert!(previewer.queued[0].is_cancelled());
        assert_eq!(previewer.cancelled_on_clear, [true]);
    }
}

#[test]
fn unwinding_cancels_pending_work_without_reentering_preview_callbacks() {
    let previewer = TestPreviewer::default();
    let mut terminal = RunnerTerminal::default();
    let mut picker = picker(["alpha"]).with_preview(previewer);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = picker
            .pick_with_terminal_io(FrameEvents(|| panic!("event source panic")), &mut terminal);
    }));
    assert!(panic.is_err());
    let previewer = picker.previewer();
    assert_eq!(terminal.cleanups, 1);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.hovered, [Some("alpha")]);
    assert!(previewer.cancelled_on_clear.is_empty());
}
