use super::*;
use crate::preview::{
    Buffer, Preview,
    request::{ActiveRequest, Request, Response},
};
use crate::session::ItemRef;

fn previewer(limit: usize) -> RefCell<TestPreviewer> {
    RefCell::new(TestPreviewer {
        prefetch_limit: limit,
        prefetch_enabled: true,
        ..TestPreviewer::default()
    })
}

#[test]
fn idle_frames_prefetch_one_neighbour_in_distance_order_within_the_deadline() {
    let driver = driver(["a", "b", "c", "d", "e", "f"]);
    let previewer = previewer(3);
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(2));
    let mut state = interface.init(&cx);
    assert_eq!(
        frame(&mut interface, &mut state, &cx, Instant::now()),
        Components::PREVIEW
    );
    assert!(previewer.borrow().prefetched.is_empty());
    for count in 1..=5 {
        assert!(frame(&mut interface, &mut state, &cx, deadline()).is_none());
        assert_eq!(previewer.borrow().prefetched.len(), count.min(3));
    }
    assert_eq!(previewer.borrow().prefetched, ["d", "b", "e"]);
    assert_eq!(previewer.borrow().requested, ["c"]);
    assert_eq!(text(interface.preview_content(&state)), "c");
}

#[test]
fn queued_work_gates_prefetch_and_started_work_is_reused_on_hover() {
    let driver = driver(["a", "b", "c", "d", "e"]);
    let previewer = previewer(4);
    previewer.borrow_mut().defer = true;
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(2));
    let mut state = interface.init(&cx);
    for _ in 0..2 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
    }
    assert!(previewer.borrow().prefetched.is_empty());
    let foreground = previewer.borrow_mut().take_request("c").start().unwrap();
    for _ in 0..2 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
    }
    assert_eq!(previewer.borrow().prefetched, ["d"]);
    let neighbour = previewer.borrow_mut().take_request("d").start().unwrap();
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    assert_eq!(previewer.borrow().prefetched, ["d", "b"]);

    let next = context(&driver, Some(3));
    let _ = before(&mut interface, &mut state, &next);
    assert!(!neighbour.is_cancelled());
    assert!(previewer.borrow().queued[0].is_cancelled());
    publish(neighbour);
    assert_eq!(
        before(&mut interface, &mut state, &next),
        Components::PREVIEW
    );
    assert_eq!(text(interface.preview_content(&state)), "d");
    publish(foreground);
    let _ = before(&mut interface, &mut state, &cx);
    assert_eq!(text(interface.preview_content(&state)), "c");
    assert_eq!(previewer.borrow().requested, ["c"]);
}

#[test]
fn hovering_a_queued_prefetch_promotes_it_to_a_foreground_request() {
    let driver = driver(["a", "b"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    let queued = previewer.borrow_mut().take_request("b");
    previewer.borrow_mut().defer = true;
    let next = context(&driver, Some(1));
    let _ = before(&mut interface, &mut state, &next);
    assert!(queued.is_cancelled());
    assert_eq!(previewer.borrow().requested, ["a", "b"]);
    publish(previewer.borrow_mut().take_request("b").start().unwrap());
    let _ = before(&mut interface, &mut state, &next);
    assert_eq!(text(interface.preview_content(&state)), "b");
}

#[test]
fn refresh_invalidates_a_prefetched_result_before_it_is_displayed() {
    let driver = driver(["a", "b"]);
    let previewer = previewer(1);
    let mut interface = CachedPreview::new_single(&previewer);
    let current = context(&driver, Some(0));
    let next = context(&driver, Some(1));
    let mut state = interface.init(&current);
    let _ = frame(&mut interface, &mut state, &current, deadline());
    assert_eq!(previewer.borrow().prefetched, ["b"]);
    let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &next);
    assert!(before(&mut interface, &mut state, &current).is_none());
    assert_eq!(previewer.borrow().requested, ["a"]);
    previewer.borrow_mut().lines = 2;
    let _ = before(&mut interface, &mut state, &next);
    assert_eq!(
        ready(interface.preview_content(&state))
            .buffer()
            .lines()
            .len(),
        2
    );
    assert_eq!(previewer.borrow().requested, ["a", "b"]);
}

#[test]
fn filtering_replaces_stale_neighbours_without_regenerating_the_current_preview() {
    let mut driver = driver(["alpha", "beta", "alpine", "alphabet"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    let outdated = previewer.borrow_mut().take_request("beta");

    driver.update_query("al");
    driver.filter().unwrap();
    let cx = context(&driver, Some(0));
    assert!(
        interface
            .before_frame(&mut state, &cx, true, deadline())
            .is_none()
    );
    assert!(outdated.is_cancelled());
    let _ = interface.after_frame(&mut state, &cx, deadline());
    assert_eq!(previewer.borrow().prefetched, ["beta", "alpine"]);
    publish(
        previewer
            .borrow_mut()
            .take_request("alpine")
            .start()
            .unwrap(),
    );
    let _ = before(&mut interface, &mut state, &context(&driver, Some(1)));
    assert_eq!(text(interface.preview_content(&state)), "alpine");
    assert_eq!(previewer.borrow().requested, ["alpha"]);
}

#[test]
fn dropped_prefetches_retry_after_navigation_instead_of_every_idle_frame() {
    let driver = driver(["a", "b", "c"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    drop(previewer.borrow_mut().take_request("b").start().unwrap());
    for _ in 0..2 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
    }
    assert_eq!(previewer.borrow().prefetched, ["b"]);
    let _ = before(&mut interface, &mut state, &context(&driver, Some(2)));
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    assert_eq!(previewer.borrow().prefetched, ["b", "b"]);
    publish(previewer.borrow_mut().take_request("b").start().unwrap());
    let _ = before(&mut interface, &mut state, &context(&driver, Some(1)));
    assert_eq!(text(interface.preview_content(&state)), "b");
    assert_eq!(previewer.borrow().requested, ["a", "c"]);
}

#[test]
fn match_changes_while_hidden_allow_declined_prefetches_to_retry_on_reveal() {
    let mut driver = driver(["a", "b", "c"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_enabled = false;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    for _ in 0..2 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
    }
    assert_eq!(previewer.borrow().prefetched, ["b"]);
    driver.push_batch(["d"]);
    driver.filter().unwrap();
    let hidden = context(&driver, Some(0)).with_preview_enabled(false);
    let _ = interface.before_frame(&mut state, &hidden, true, deadline());
    let _ = frame(&mut interface, &mut state, &hidden, deadline());
    assert_eq!(previewer.borrow().prefetched, ["b"]);

    previewer.borrow_mut().prefetch_enabled = true;
    let cx = context(&driver, Some(0));
    for _ in 0..2 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
    }
    assert_eq!(previewer.borrow().prefetched, ["b", "b"]);
    let _ = before(&mut interface, &mut state, &context(&driver, Some(1)));
    assert_eq!(text(interface.preview_content(&state)), "b");
    assert_eq!(previewer.borrow().requested, ["a"]);
}

#[test]
fn declined_prefetch_preserves_previous_results_and_active_requests() {
    let driver = driver(["a", "b", "c", "d", "e", "f"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let mut state = interface.init(&context(&driver, Some(0)));
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(0)),
        deadline(),
    );
    publish(previewer.borrow_mut().take_request("b").start().unwrap());
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(2)),
        deadline(),
    );
    let active = previewer.borrow_mut().take_request("d").start().unwrap();
    previewer.borrow_mut().prefetch_enabled = false;
    for _ in 0..2 {
        let _ = frame(
            &mut interface,
            &mut state,
            &context(&driver, Some(4)),
            deadline(),
        );
    }
    assert_eq!(previewer.borrow().prefetched, ["b", "d", "f"]);
    assert!(!active.is_cancelled());
    publish(active);
    for (rank, item) in [(1, "b"), (3, "d")] {
        let _ = before(&mut interface, &mut state, &context(&driver, Some(rank)));
        assert_eq!(text(interface.preview_content(&state)), item);
    }
    assert_eq!(previewer.borrow().requested, ["a", "c", "e"]);
}

#[test]
fn checking_cached_neighbours_preserves_main_cache_recency_and_scroll() {
    let driver = driver(["a", "b", "c", "d"]);
    let previewer = previewer(1);
    previewer.borrow_mut().lines = 20;
    let mut interface = CachedPreview::new_single(&previewer).cache_size(NonZero::new(3));
    let mut state = interface.init(&context(&driver, Some(1)));
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 12,
            height: 5,
        }),
    );
    for rank in [1, 2, 0] {
        let cx = context(&driver, Some(rank));
        let _ = before(&mut interface, &mut state, &cx);
        let _ = interface.preview_event(&mut state, PreviewEvent::Down(7), &cx);
    }
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(0)),
        deadline(),
    );
    assert!(previewer.borrow().prefetched.is_empty());
    for rank in [3, 2] {
        let _ = before(&mut interface, &mut state, &context(&driver, Some(rank)));
    }
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 7));
    let _ = before(&mut interface, &mut state, &context(&driver, Some(1)));
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 0));
    assert_eq!(previewer.borrow().requested, ["b", "c", "a", "d", "b"]);
}

#[test]
fn hover_callback_can_start_a_prefetch_before_stale_work_is_cancelled() {
    struct StartOnHover<'a> {
        previewer: &'a RefCell<TestPreviewer>,
        active: &'a RefCell<Option<ActiveRequest<&'static str>>>,
    }

    impl Preview<&'static str> for StartOnHover<'_> {
        fn prefetch_limit(&self) -> usize {
            1
        }

        fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
            self.previewer.preview(request, timeout)
        }

        fn prefetch(
            &mut self,
            request: Request<'_, &'static str>,
            timeout: Duration,
        ) -> Result<Response, Buffer> {
            self.previewer.prefetch(request, timeout)
        }

        fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
            self.previewer.hover_changed(item);
            if item.is_some_and(|item| *item.value() == "beta") {
                *self.active.borrow_mut() = Some(
                    self.previewer
                        .borrow_mut()
                        .take_request("beta")
                        .start()
                        .unwrap(),
                );
            }
        }
    }

    let mut driver = driver(["alpha", "beta", "gamma"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let active = RefCell::new(None);
    let mut interface = CachedPreview::new_single(StartOnHover {
        previewer: &previewer,
        active: &active,
    });
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    driver.update_query("beta");
    driver.filter().unwrap();
    let cx = context(&driver, Some(0));
    let _ = interface.before_frame(&mut state, &cx, true, deadline());
    let active = active.borrow_mut().take().unwrap();
    assert!(!active.is_cancelled());
    publish(active);
    let _ = before(&mut interface, &mut state, &cx);
    assert_eq!(text(interface.preview_content(&state)), "beta");
    assert_eq!(previewer.borrow().requested, ["alpha"]);
}

#[test]
fn immediately_dropped_prefetches_are_attempted_once_per_neighbour() {
    let driver = driver(["a", "b", "c", "d"]);
    let previewer = previewer(3);
    previewer.borrow_mut().prefetch_defer = true;
    previewer.borrow_mut().drop_requests = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    for count in 1..=5 {
        let _ = frame(&mut interface, &mut state, &cx, deadline());
        assert_eq!(previewer.borrow().prefetched.len(), count.min(3));
    }
    assert_eq!(previewer.borrow().prefetched, ["b", "c", "d"]);
}

#[test]
fn prefetch_configuration_is_sampled_at_init_and_retained_across_restart() {
    let driver = driver(["a", "b"]);
    let previewer = previewer(1);
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    previewer.borrow_mut().prefetch_limit = 0;
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    interface.on_restart(&mut state, &cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    assert_eq!(previewer.borrow().prefetched, ["b", "b"]);
    interface.on_end(&mut state);
    let mut state = interface.init(&cx);
    let _ = frame(&mut interface, &mut state, &cx, deadline());
    assert_eq!(previewer.borrow().prefetched, ["b", "b"]);
}

#[test]
fn prefetch_eviction_cancels_active_work() {
    let driver = driver(["a", "b", "c", "d", "e", "f"]);
    let previewer = previewer(1);
    previewer.borrow_mut().prefetch_defer = true;
    let mut interface = CachedPreview::new_single(&previewer);
    let mut state = interface.init(&context(&driver, Some(0)));
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(0)),
        deadline(),
    );
    let first = previewer.borrow_mut().take_request("b").start().unwrap();
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(2)),
        deadline(),
    );
    let second = previewer.borrow_mut().take_request("d").start().unwrap();
    let _ = frame(
        &mut interface,
        &mut state,
        &context(&driver, Some(4)),
        deadline(),
    );
    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
}
