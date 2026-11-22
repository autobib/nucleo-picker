use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use crate::{
    interface::Interface,
    preview::{
        Buffer, Content, Pane, Preview,
        request::{ActiveRequest, QueuedRequest, Request, Response},
    },
    render::StrRenderer,
    session::{Context, Driver, DriverOptions, ItemId, ItemRef, Redraw},
};

pub(crate) fn driver(
    items: impl IntoIterator<Item = &'static str>,
) -> Driver<&'static str, StrRenderer> {
    let mut driver = DriverOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .driver(StrRenderer);
    driver.push_batch(items);
    driver.filter().unwrap();
    driver
}

pub(crate) fn context<'a>(
    driver: &'a Driver<&'static str, StrRenderer>,
    rank: Option<u32>,
) -> Context<'a, &'static str> {
    Context::new(driver.matches())
        .with_rank(rank)
        .with_preview_enabled(true)
}

pub(crate) fn ready(content: Content<'_>) -> &Pane {
    match content {
        Content::Ready(pane) => pane,
        _ => panic!("expected ready preview"),
    }
}

pub(crate) fn text(content: Content<'_>) -> &str {
    ready(content).buffer().line(0).unwrap().as_str()
}

pub(crate) fn before<I: Interface<&'static str>>(
    interface: &mut I,
    state: &mut I::State,
    cx: &Context<'_, &'static str>,
) -> Redraw {
    interface.before_frame(state, cx, false, Instant::now())
}

pub(crate) fn frame<I: Interface<&'static str>>(
    interface: &mut I,
    state: &mut I::State,
    cx: &Context<'_, &'static str>,
    deadline: Instant,
) -> Redraw {
    let redraw = interface.before_frame(state, cx, false, deadline);
    assert_eq!(interface.after_frame(state, cx, deadline), Redraw::NONE);
    redraw
}

pub(crate) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

pub(crate) fn publish(request: ActiveRequest<&'static str>) {
    let mut pane = Pane::new();
    pane.push_text(request.item());
    request.publish(&mut pane).unwrap();
}

#[derive(Default)]
pub(crate) struct TestPreviewer {
    pub(crate) hovered: Vec<Option<&'static str>>,
    pub(crate) cancelled_on_clear: Vec<bool>,
    pub(crate) requested: Vec<&'static str>,
    pub(crate) ids: Vec<ItemId>,
    pub(crate) prefetched: Vec<&'static str>,
    pub(crate) queued: Vec<QueuedRequest<&'static str>>,
    pub(crate) restarts: usize,
    pub(crate) defer: bool,
    pub(crate) prefetch_limit: usize,
    pub(crate) prefetch_enabled: bool,
    pub(crate) prefetch_defer: bool,
    pub(crate) drop_requests: bool,
    pub(crate) lines: usize,
}

impl TestPreviewer {
    pub(crate) fn take_request(&mut self, item: &str) -> QueuedRequest<&'static str> {
        let index = self
            .queued
            .iter()
            .position(|request| *request.item() == item && !request.is_cancelled())
            .expect("expected queued request for item");
        self.queued.remove(index)
    }

    fn respond(&mut self, request: Request<'_, &'static str>, defer: bool) -> Response {
        if defer {
            let (pending, queued) = request.defer();
            if !self.drop_requests {
                self.queued.push(queued);
            }
            Response::Pending(pending)
        } else {
            let item = request.item();
            let mut pane = request.ready();
            assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
            assert_eq!(pane.offset(), (0, 0));
            assert!(!pane.line_numbers());
            assert!(!pane.is_err());
            pane.push_text(item);
            for _ in 1..self.lines {
                pane.push_text("\n");
                pane.push_text(item);
            }
            Response::Ready(pane)
        }
    }
}

impl Preview<&'static str> for TestPreviewer {
    fn prefetch_limit(&self) -> usize {
        self.prefetch_limit
    }

    fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
        assert!(timeout >= Duration::from_millis(2));
        assert_eq!(self.hovered.last(), Some(&Some(*request.item())));
        self.requested.push(request.item());
        self.ids.push(request.id());
        self.respond(request, self.defer)
    }

    fn prefetch(
        &mut self,
        request: Request<'_, &'static str>,
        timeout: Duration,
    ) -> Result<Response, Buffer> {
        assert!(!timeout.is_zero());
        self.prefetched.push(request.item());
        if self.prefetch_enabled {
            Ok(self.respond(request, self.prefetch_defer))
        } else {
            Err(request.ready().into_buffer())
        }
    }

    fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
        if item.is_none() {
            self.cancelled_on_clear
                .push(self.queued.iter().all(QueuedRequest::is_cancelled));
        }
        self.hovered.push(item.map(|item| *item.value()));
    }

    fn restart(&mut self) {
        assert!(self.queued.iter().all(QueuedRequest::is_cancelled));
        assert!(self.hovered.last().is_none_or(Option::is_none));
        self.restarts += 1;
    }
}

impl Preview<&'static str> for &RefCell<TestPreviewer> {
    fn prefetch_limit(&self) -> usize {
        self.borrow().prefetch_limit()
    }

    fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
        self.borrow_mut().preview(request, timeout)
    }

    fn prefetch(
        &mut self,
        request: Request<'_, &'static str>,
        timeout: Duration,
    ) -> Result<Response, Buffer> {
        self.borrow_mut().prefetch(request, timeout)
    }

    fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
        self.borrow_mut().hover_changed(item);
    }

    fn restart(&mut self) {
        self.borrow_mut().restart();
    }
}

#[cfg(feature = "unstable-backend")]
pub(crate) fn picker(
    items: impl IntoIterator<Item = &'static str>,
) -> crate::Picker<&'static str, StrRenderer> {
    let mut picker = crate::PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    picker.filter().unwrap();
    picker
}
