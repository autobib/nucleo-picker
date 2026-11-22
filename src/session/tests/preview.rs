use std::{
    cell::{Cell, RefCell},
    io,
    time::Duration,
};

use super::{FrameEvents, TestTerminal, frame_events, frame_options};
use crate::{
    PickerOptions, Terminal,
    error::PickError,
    event::{Event, MatchListEvent, RecvError},
    interface::{CachedPreview, controller::tests::support::*},
    preview::{
        Buffer, Pane, Preview, PreviewEvent, SyncPreviewer,
        request::{Request, Response},
    },
    rect::{Area, ClearState, CrosstermTarget},
    render::StrRenderer,
    session::Dimensions,
};

#[test]
fn buffered_navigation_targets_cached_event_items_without_generating_unseen_previews() {
    let mut driver = driver(["alpha", "beta", "unseen"]);
    let generated = RefCell::new(Vec::new());
    let previewer = SyncPreviewer(|item: &&'static str, pane: &mut Pane| {
        generated.borrow_mut().push(*item);
        for line in 0..30 {
            if line != 0 {
                pane.push_text("\n");
            }
            pane.push_text(&format!("{item}{line:02}"));
        }
    });
    let mut session = driver.session(CachedPreview::new_single(previewer));
    let area = Area {
        width: 30,
        height: 10,
        ..Area::default()
    };
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions {
        width: area.width,
        height: area.height,
    });
    initial.complete();
    session.events().match_list(MatchListEvent::Up(1));
    session.prepare_frame(frame_options()).complete();
    {
        let mut events = session.events();
        events.preview(PreviewEvent::PageDown(1));
        events.match_list(MatchListEvent::Up(1));
        events.preview(PreviewEvent::Down(5));
        events.match_list(MatchListEvent::Down(2));
        events.preview(PreviewEvent::Down(3));
    }
    let mut frame = session.prepare_frame(frame_options());
    let mut terminal = TestTerminal::default();
    frame
        .preview()
        .draw(&mut CrosstermTarget::new(&mut terminal).rect(area, ClearState::WithinRect))
        .unwrap();
    assert!(
        String::from_utf8(terminal.output)
            .unwrap()
            .contains("alpha03")
    );
    frame.complete();

    session.events().match_list(MatchListEvent::Up(1));
    let mut frame = session.prepare_frame(frame_options());
    let mut terminal = TestTerminal::default();
    frame
        .preview()
        .draw(&mut CrosstermTarget::new(&mut terminal).rect(area, ClearState::WithinRect))
        .unwrap();
    assert!(
        String::from_utf8(terminal.output)
            .unwrap()
            .contains("beta10")
    );
    frame.complete();
    assert_eq!(*generated.borrow(), ["alpha", "beta"]);

    {
        let mut events = session.events();
        events.match_list(MatchListEvent::Down(1));
        events.preview(PreviewEvent::Refresh);
        events.match_list(MatchListEvent::Up(1));
    }
    session.prepare_frame(frame_options()).complete();
    assert_eq!(*generated.borrow(), ["alpha", "beta"]);
    session.events().match_list(MatchListEvent::Down(1));
    session.prepare_frame(frame_options()).complete();
    assert_eq!(*generated.borrow(), ["alpha", "beta", "alpha"]);
}

#[derive(Default)]
struct RunnerTerminal<'a> {
    renders: Option<&'a Cell<usize>>,
    size: Option<&'a Cell<(u16, u16)>>,
    cleanups: usize,
    changed: Vec<bool>,
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
        let (width, height) = self.size.map_or((20, 10), Cell::get);
        Ok(Dimensions { width, height })
    }

    fn end_render(&mut self) -> io::Result<()> {
        if let Some(renders) = self.renders {
            renders.set(renders.get() + 1);
        }
        Ok(())
    }

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.changed.push(changed);
        Ok(())
    }
}

#[test]
fn prefetch_follows_rendering_and_progresses_on_idle_frames() {
    struct Previewer<'a> {
        renders: &'a Cell<usize>,
        prefetched: Vec<(&'static str, usize)>,
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
            timeout: Duration,
        ) -> Result<Response, Buffer> {
            self.prefetched.push((*request.item(), self.renders.get()));
            Ok(self.preview(request, timeout))
        }
    }

    let renders = Cell::new(0);
    let mut previewer = Previewer {
        renders: &renders,
        prefetched: Vec::new(),
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
    picker
        .with_preview(&mut previewer)
        .pick_with_terminal_io(
            frame_events([
                Err(RecvError::Timeout),
                Ok(Event::MatchList(MatchListEvent::Up(1))),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]),
            &mut terminal,
        )
        .unwrap();
    assert_eq!(previewer.prefetched, [("b", 1), ("c", 1), ("d", 2)]);
    assert_eq!(terminal.changed, [false, true]);
}

#[test]
fn terminal_size_is_queried_after_preview_generation() {
    let mut picker = picker(["alpha", "beta"]);
    let statuses = picker.status_observer();
    let size = Cell::new((20, 10));
    let mut terminal = RunnerTerminal {
        size: Some(&size),
        ..RunnerTerminal::default()
    };
    let previewer = SyncPreviewer(|item: &&str, pane: &mut Pane| {
        if *item == "beta" {
            size.set((30, 12));
        }
        pane.push_text(item);
    });
    picker
        .with_preview(previewer)
        .pick_with_terminal_io(
            frame_events([
                Ok(Event::MatchList(MatchListEvent::Up(1))),
                Ok(Event::Status { id: 0 }),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]),
            &mut terminal,
        )
        .unwrap();
    assert_eq!(
        statuses.try_recv().unwrap().dimensions,
        Dimensions {
            width: 30,
            height: 12
        }
    );
}

#[test]
fn refresh_then_quit_cancels_work_without_generating_a_replacement() {
    let mut previewer = TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    };
    let mut terminal = RunnerTerminal::default();
    picker(["alpha"])
        .with_preview(&mut previewer)
        .pick_with_terminal_io(
            frame_events([Ok(Event::Preview(PreviewEvent::Refresh)), Ok(Event::Quit)]),
            &mut terminal,
        )
        .unwrap();
    assert_eq!(previewer.requested, ["alpha"]);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.hovered, [Some("alpha"), None]);
    assert_eq!(terminal.cleanups, 1);
}

#[test]
fn presentation_and_cleanup_errors_cancel_pending_previews() {
    for fail_write in [false, true] {
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        let mut terminal = RunnerTerminal {
            fail_write,
            fail_cleanup: !fail_write,
            ..RunnerTerminal::default()
        };
        let mut picker = picker(["alpha"]).with_preview(&mut previewer);
        let result = picker.pick_with_terminal_io(frame_events([Ok(Event::Quit)]), &mut terminal);
        assert!(matches!(result, Err(PickError::IO(_))));
        drop(picker);
        assert_eq!(terminal.cleanups, 1);
        assert!(previewer.queued[0].is_cancelled());
        assert_eq!(previewer.cancelled_on_clear, [true]);
    }
}

#[test]
fn unwinding_cancels_pending_work_without_reentering_preview_callbacks() {
    let mut previewer = TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    };
    let mut terminal = RunnerTerminal::default();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = picker(["alpha"])
            .with_preview(&mut previewer)
            .pick_with_terminal_io(FrameEvents(|| panic!("event source panic")), &mut terminal);
    }));
    assert!(panic.is_err());
    assert_eq!(terminal.cleanups, 1);
    assert!(previewer.queued[0].is_cancelled());
    assert_eq!(previewer.hovered, [Some("alpha")]);
    assert!(previewer.cancelled_on_clear.is_empty());
}
