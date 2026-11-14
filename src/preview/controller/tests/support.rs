use super::super::*;
use crate::{
    Picker, PickerOptions,
    match_engine::MatchEngine,
    preview::{
        DEFAULT_CACHE_SIZE, ItemId, ItemRef,
        request::{ActiveRequest, QueuedRequest, Response},
    },
    render::StrRenderer,
};

#[derive(Default)]
pub(crate) struct TestPreviewer {
    pub(crate) hovered: Vec<Option<&'static str>>,
    pub(crate) hovered_ids: Vec<Option<ItemId>>,
    pub(crate) cancelled_on_clear: Vec<bool>,
    pub(crate) requested: Vec<&'static str>,
    pub(crate) ids: Vec<ItemId>,
    pub(crate) buffer_allocations: Vec<*const u8>,
    pub(crate) prefetched: Vec<&'static str>,
    pub(crate) prefetch_allocations: Vec<*const u8>,
    pub(crate) queued: Vec<QueuedRequest<&'static str>>,
    pub(crate) restarts: usize,
    pub(crate) defer: bool,
    pub(crate) prefetch_limit: usize,
    pub(crate) prefetch_enabled: bool,
    pub(crate) prefetch_defer: bool,
    pub(crate) drop_requests: bool,
    pub(crate) lines: usize,
    pub(crate) line_numbers: bool,
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
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Stage {
    Queued,
    Active,
    Published,
    DroppedQueued,
    DroppedActive,
}

impl Stage {
    pub(crate) const ALL: [Self; 5] = [
        Self::Queued,
        Self::Active,
        Self::Published,
        Self::DroppedQueued,
        Self::DroppedActive,
    ];

    pub(crate) fn prepare(
        self,
        request: QueuedRequest<&'static str>,
    ) -> (
        Option<QueuedRequest<&'static str>>,
        Option<ActiveRequest<&'static str>>,
    ) {
        match self {
            Self::Queued => return (Some(request), None),
            Self::Active => return (None, Some(request.start().unwrap())),
            Self::Published => publish(request.start().unwrap()),
            Self::DroppedQueued => drop(request),
            Self::DroppedActive => drop(request.start().unwrap()),
        }
        (None, None)
    }
}

pub(crate) fn publish(request: ActiveRequest<&'static str>) {
    let mut buffer = Buffer::new();
    buffer.push_str(request.item());
    request.publish(&mut buffer.into_pane()).unwrap();
}

pub(crate) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

impl<T: Send + Sync + 'static, P: Preview<T>> PreviewController<T, P> {
    pub(crate) fn run_frame<R>(
        &mut self,
        rank: Option<u32>,
        engine: &MatchEngine<T, R>,
        deadline: Instant,
    ) -> bool {
        let cx = Context::new(engine)
            .with_rank(rank)
            .with_preview(Some(interior_dimensions(self.state.area)));
        let redraw = self.update(&cx, deadline);
        self.prefetch(&cx, deadline);
        redraw
    }

    pub(crate) fn update_at<R>(
        &mut self,
        rank: Option<u32>,
        engine: &MatchEngine<T, R>,
        deadline: Instant,
    ) -> bool {
        let cx = Context::new(engine)
            .with_rank(rank)
            .with_preview(Some(interior_dimensions(self.state.area)));
        self.update(&cx, deadline)
    }

    pub(crate) fn handle_at(&mut self, event: PreviewEvent, hovered: Option<u32>) {
        self.state
            .handle_event(hovered, event, interior_dimensions(self.state.area));
    }
}

impl<T, P: Preview<T>> PreviewController<T, P> {
    pub(crate) fn contents(&self) -> Vec<&str> {
        let Some(RequestState::Ready(pane)) = &self.state.cached().unwrap().state else {
            panic!("expected ready preview");
        };
        pane.buffer().lines().map(|line| line.as_str()).collect()
    }
}

impl Preview<&'static str> for TestPreviewer {
    fn prefetch_limit(&self) -> usize {
        self.prefetch_limit
    }

    fn prefetch(
        &mut self,
        request: Request<'_, &'static str>,
        timeout: Duration,
    ) -> Result<Response, Buffer> {
        assert!(!timeout.is_zero());
        let item = request.item();
        self.prefetched.push(item);
        assert_eq!(request.buffer.lines().len(), 1);
        assert_eq!(request.buffer.line(0).unwrap().as_str(), "");
        self.prefetch_allocations
            .push(request.buffer.line(0).unwrap().as_str().as_ptr());
        if !self.prefetch_enabled {
            return Err(request.ready().into_buffer());
        }
        if self.prefetch_defer {
            let (pending, queued) = request.defer();
            if !self.drop_requests {
                self.queued.push(queued);
            }
            Ok(Response::Pending(pending))
        } else {
            let mut pane = request.ready();
            assert!(!pane.is_err());
            pane.buffer_mut().push_str(item);
            Ok(Response::Ready(pane))
        }
    }

    fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
        if item.is_none() {
            self.cancelled_on_clear
                .push(self.queued.iter().all(QueuedRequest::is_cancelled));
        }
        self.hovered.push(item.map(|item| *item.value()));
        self.hovered_ids.push(item.map(|item| item.id()));
    }

    fn restart(&mut self) {
        assert!(self.queued.iter().all(QueuedRequest::is_cancelled));
        assert!(self.hovered.last().is_none_or(Option::is_none));
        self.restarts += 1;
    }

    fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
        assert!(timeout >= Duration::from_millis(2));
        let item = request.item();
        assert_eq!(self.hovered.last(), Some(&Some(*item)));
        assert_eq!(self.hovered_ids.last(), Some(&Some(request.id())));
        self.requested.push(item);
        self.ids.push(request.id());
        assert_eq!(request.buffer.lines().len(), 1);
        assert_eq!(request.buffer.line(0).unwrap().as_str(), "");
        self.buffer_allocations
            .push(request.buffer.line(0).unwrap().as_str().as_ptr());
        if self.defer {
            let (pending, queued) = request.defer();
            assert_eq!(self.ids.last(), Some(&queued.id()));
            if !self.drop_requests {
                self.queued.push(queued);
            }
            Response::Pending(pending)
        } else {
            let mut pane = request.ready();
            assert!(!pane.is_err());
            let buffer = pane.buffer_mut();
            buffer.push_str(item);
            for _ in 1..self.lines {
                buffer.newline();
                buffer.push_str(item);
            }
            Response::Ready(pane.with_line_numbers(self.line_numbers))
        }
    }
}

pub(crate) fn settle<I>(picker: &mut Picker<&'static str, StrRenderer, I>) {
    let start = std::time::Instant::now();
    loop {
        let status = picker.core.engine.update(5);
        if status.items_changed {
            picker.core.list_state.layout.update_items(
                picker.core.engine.snapshot(),
                &picker.core.list_state.config,
            );
        }
        if !status.matching {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}

pub(crate) fn picker(
    items: impl IntoIterator<Item = &'static str>,
) -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    settle(&mut picker);
    picker.core.list_state.layout.resize(
        picker.core.engine.snapshot(),
        8,
        &picker.core.list_state.config,
    );
    picker
}

pub(crate) fn hovered_index(picker: &Picker<&'static str, StrRenderer>) -> Option<u32> {
    picker
        .core
        .list_state
        .layout
        .rank(picker.core.engine.snapshot())
        .map(|n| picker.core.engine.idx_from_match(n))
}

pub(crate) fn preview_pane<P: Preview<&'static str>>(
    previewer: P,
) -> PreviewController<&'static str, P> {
    PreviewController::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        DEFAULT_CACHE_SIZE,
        previewer,
    )
}

pub(crate) fn update(
    application: &mut PreviewController<&'static str, TestPreviewer>,
    picker: &mut Picker<&'static str, StrRenderer>,
) -> bool {
    let rank = picker
        .core
        .list_state
        .layout
        .rank(picker.core.engine.snapshot());
    application.update_at(rank, &picker.core.engine, Instant::now())
}

#[cfg(feature = "unstable-backend")]
impl<T, P: Preview<T>> PreviewController<T, P> {
    pub(crate) fn test_cache(&mut self) -> &mut LruCache<u32, Cached> {
        &mut self.state.cache
    }

    pub(crate) fn test_previewer(&mut self) -> &mut P {
        &mut self.previewer
    }
}
