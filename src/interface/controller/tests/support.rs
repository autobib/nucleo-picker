use super::super::*;
use crate::{
    Picker, PickerOptions,
    interface::{CachedPreview, CachedPreviewState, Single},
    preview::{
        DEFAULT_CACHE_SIZE, ItemId, ItemRef, interior_dimensions,
        request::{ActiveRequest, QueuedRequest, Response},
    },
    rect::Area,
    render::StrRenderer,
    session::{Exit, Interface, Redraw},
};

pub(crate) struct TestSession<P> {
    // Drop cached requests before the previewer, including during unwinding.
    pub(super) state: CachedPreviewState<()>,
    pub(super) interface: CachedPreview<Single, P>,
}

impl<P: Preview<&'static str>> TestSession<P> {
    pub(crate) fn new(cache_size: Option<NonZero<usize>>, previewer: P) -> Self {
        Self {
            state: CachedPreviewState {
                interface: (),
                preview: PreviewState::new(cache_size, previewer.prefetch_limit()),
            },
            interface: CachedPreview::new_single(previewer).cache_size(cache_size),
        }
    }

    pub(crate) fn resize(&mut self, area: Area) {
        self.preview_resize(&mut (), Some(interior_dimensions(area)));
    }

    pub(crate) fn handle(&mut self, event: PreviewEvent, cx: &Context<'_, &'static str>) -> bool {
        self.preview_event(&mut (), event, cx).preview
    }

    pub(crate) fn update(
        &mut self,
        cx: &Context<'_, &'static str>,
        matches_changed: bool,
        deadline: Instant,
    ) -> bool {
        self.before_frame(&mut (), cx, matches_changed, deadline)
            .preview
    }

    pub(crate) fn prefetch(&mut self, cx: &Context<'_, &'static str>, deadline: Instant) {
        let _ = self.after_frame(&mut (), cx, deadline);
    }

    pub(crate) fn hide(&mut self) {
        self.state
            .preview
            .set_hovered(&mut self.interface.previewer, None);
    }

    pub(crate) fn restart(&mut self) {
        self.state.preview.restart(&mut self.interface.previewer);
    }

    pub(crate) fn end(&mut self) {
        self.interface.on_end(&mut self.state);
    }
}

impl<P: Preview<&'static str>> Interface<&'static str> for TestSession<P> {
    // Event-loop tests retain previews between runs and inspect state before explicit cleanup.
    type State = ();
    type Output<'items> = Option<&'items &'static str>;

    const PREVIEW: bool = true;

    fn init(&mut self, _: &Context<'_, &'static str>) {}

    fn on_restart(&mut self, _: &mut (), cx: &Context<'_, &'static str>) {
        self.interface.on_restart(&mut self.state, cx);
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, &'static str>,
    ) -> Self::Output<'items> {
        Single.finish((), exit, cx)
    }
    fn preview_content<'a>(&'a self, _: &'a ()) -> Content<'a> {
        self.interface.preview_content(&self.state)
    }

    fn preview_resize(&mut self, _: &mut (), size: Option<Dimensions>) {
        self.interface.preview_resize(&mut self.state, size);
    }

    fn preview_event(
        &mut self,
        _: &mut (),
        event: PreviewEvent,
        cx: &Context<'_, &'static str>,
    ) -> Redraw {
        self.interface.preview_event(&mut self.state, event, cx)
    }

    fn before_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, &'static str>,
        matches_changed: bool,
        deadline: Instant,
    ) -> Redraw {
        self.interface
            .before_frame(&mut self.state, cx, matches_changed, deadline)
    }

    fn after_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, &'static str>,
        deadline: Instant,
    ) -> Redraw {
        self.interface.after_frame(&mut self.state, cx, deadline)
    }
}

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

impl<P: Preview<&'static str>> TestSession<P> {
    pub(crate) fn run_frame(
        &mut self,
        rank: Option<u32>,
        matches: Matches<'_, &'static str>,
        deadline: Instant,
    ) -> bool {
        let cx = Context::new(matches)
            .with_rank(rank)
            .with_preview_enabled(true);
        let redraw = self.update(&cx, false, deadline);
        self.prefetch(&cx, deadline);
        redraw
    }

    pub(crate) fn update_at(
        &mut self,
        rank: Option<u32>,
        matches: Matches<'_, &'static str>,
        deadline: Instant,
    ) -> bool {
        let cx = Context::new(matches)
            .with_rank(rank)
            .with_preview_enabled(true);
        self.update(&cx, false, deadline)
    }

    pub(crate) fn handle_at(&mut self, event: PreviewEvent, hovered: Option<ItemId>) -> bool {
        self.state.preview.handle_event(hovered, event)
    }
}

impl<P> TestSession<P> {
    pub(crate) fn contents(&self) -> Vec<&str> {
        let Some(RequestState::Ready(pane)) = &self.state.preview.cached().unwrap().state else {
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
        let status = picker.update_matches(Duration::from_millis(5));
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
    picker
}

pub(crate) fn id_at<I>(picker: &Picker<&'static str, StrRenderer, I>, rank: u32) -> ItemId {
    picker.matches().get(rank as usize).unwrap().id()
}

pub(crate) fn preview_pane<P: Preview<&'static str>>(previewer: P) -> TestSession<P> {
    TestSession::new(DEFAULT_CACHE_SIZE, previewer)
}

pub(crate) fn update(
    application: &mut TestSession<TestPreviewer>,
    picker: &mut Picker<&'static str, StrRenderer>,
) -> bool {
    let matches = picker.matches();
    let rank = (!matches.is_empty()).then_some(0);
    application.update_at(rank, matches, Instant::now())
}

#[cfg(feature = "unstable-backend")]
impl<P> TestSession<P> {
    pub(crate) fn test_cache(&mut self) -> &mut LruCache<ItemId, Cached> {
        &mut self.state.preview.cache
    }

    pub(crate) fn test_previewer(&mut self) -> &mut P {
        &mut self.interface.previewer
    }
}
