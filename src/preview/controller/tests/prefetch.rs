use super::*;
use crate::preview::SyncPreviewer;

fn pane(count: usize) -> TestSession<TestPreviewer> {
    preview_pane(TestPreviewer {
        prefetch_limit: count,
        prefetch_enabled: true,
        ..TestPreviewer::default()
    })
}

fn allocation(cached: &Cached) -> *const u8 {
    cached
        .pane()
        .unwrap()
        .buffer()
        .line(0)
        .unwrap()
        .as_str()
        .as_ptr()
}

#[test]
fn prefetch_configuration_is_sampled_once_per_session_across_restarts() {
    let picker = picker(["a", "b", "c"]);
    let mut previewer = TestPreviewer {
        prefetch_enabled: true,
        ..TestPreviewer::default()
    };
    let mut session = preview_pane(&mut previewer);
    assert!(session.state.preview.prefetch.is_none());
    for rank in [0, 1, 2, 1, 0] {
        session.run_frame(Some(rank), &picker.core.engine, deadline());
    }
    assert!(session.interface.previewer.prefetched.is_empty());
    assert_eq!(session.interface.previewer.requested, ["a", "b", "c"]);
    session.interface.previewer.prefetch_limit = 1;
    session.restart();
    assert!(session.state.preview.prefetch.is_none());
    drop(session);
    let mut session = preview_pane(&mut previewer);
    session.interface.previewer.prefetch_limit = 0;
    for _ in 0..2 {
        for _ in 0..3 {
            session.run_frame(Some(0), &picker.core.engine, deadline());
        }
        session.restart();
    }
    drop(session);
    assert_eq!(previewer.prefetched, ["b", "b"]);
    let session = preview_pane(&mut previewer as &mut dyn Preview<&'static str>);
    assert!(session.state.preview.prefetch.is_none());
}

#[test]
fn neighbours_are_prefetched_in_priority_order_without_redrawing() {
    let picker = picker(["a", "b", "c", "d", "e", "f"]);
    let mut session = pane(3);
    assert!(session.run_frame(Some(2), &picker.core.engine, Instant::now()));
    assert!(session.interface.previewer.prefetched.is_empty());
    for frame in 0..8 {
        assert!(!session.run_frame(Some(2), &picker.core.engine, deadline()));
        assert_eq!(
            session.interface.previewer.prefetched.len(),
            (frame + 1).min(3)
        );
    }
    assert_eq!(session.interface.previewer.prefetched, ["d", "b", "e"]);
    assert_eq!(session.interface.previewer.requested, ["c"]);
    assert_eq!(session.interface.previewer.hovered, [Some("c")]);
    assert_eq!(session.contents(), ["c"]);
}

#[test]
fn queued_requests_gate_prefetch_but_active_requests_do_not() {
    let picker = picker(["a", "b", "c", "d", "e"]);
    let mut session = pane(4);
    session.interface.previewer.defer = true;
    session.interface.previewer.prefetch_defer = true;
    for _ in 0..3 {
        session.run_frame(Some(2), &picker.core.engine, deadline());
    }
    assert!(session.interface.previewer.prefetched.is_empty());
    let hovered = session
        .interface
        .previewer
        .take_request("c")
        .start()
        .unwrap();

    for _ in 0..3 {
        session.run_frame(Some(2), &picker.core.engine, deadline());
    }
    assert_eq!(session.interface.previewer.prefetched, ["d"]);
    let first = session
        .interface
        .previewer
        .take_request("d")
        .start()
        .unwrap();
    session.run_frame(Some(2), &picker.core.engine, deadline());
    assert_eq!(session.interface.previewer.prefetched, ["d", "b"]);
    let second = session
        .interface
        .previewer
        .take_request("b")
        .start()
        .unwrap();
    session.run_frame(Some(2), &picker.core.engine, deadline());
    assert_eq!(session.interface.previewer.prefetched, ["d", "b", "e"]);
    assert!(!hovered.is_cancelled());
    assert!(!first.is_cancelled());
    assert!(!second.is_cancelled());
    publish(hovered);
    publish(first);
    publish(second);
    publish(
        session
            .interface
            .previewer
            .take_request("e")
            .start()
            .unwrap(),
    );
    session.run_frame(Some(2), &picker.core.engine, deadline());
    assert_eq!(session.interface.previewer.prefetched, ["d", "b", "e", "a"]);
}

#[test]
fn navigation_cancels_stale_prefetch_before_submitting_the_new_neighbour() {
    for (rank, deferred) in [(None, false), (Some(2), false), (Some(2), true)] {
        let picker = picker(["a", "b", "c", "d"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = true;
        session.run_frame(Some(0), &picker.core.engine, deadline());
        let outdated = session.interface.previewer.take_request("b");
        session.interface.previewer.defer = deferred;
        session.update_at(rank, &picker.core.engine, Instant::now());
        assert!(outdated.is_cancelled());
        assert_eq!(session.interface.previewer.prefetched, ["b"]);
        if rank.is_some() {
            if deferred {
                publish(
                    session
                        .interface
                        .previewer
                        .take_request("c")
                        .start()
                        .unwrap(),
                );
            }
            session.run_frame(rank, &picker.core.engine, deadline());
            assert_eq!(session.interface.previewer.prefetched, ["b", "d"]);
            assert!(!session.interface.previewer.take_request("d").is_cancelled());
        }
    }
}

#[test]
fn hovering_prefetched_items_reuses_started_work_and_retries_unfinished_requests() {
    for (stage, detour) in Stage::ALL
        .into_iter()
        .flat_map(|stage| [(stage, false), (stage, true)])
    {
        let picker = picker(["a", "b", "c", "d"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = true;
        session.run_frame(Some(0), &picker.core.engine, deadline());
        let (queued, active) = stage.prepare(session.interface.previewer.take_request("b"));
        let mut expected = vec!["a"];
        if detour {
            session.run_frame(Some(2), &picker.core.engine, deadline());
            expected.push("c");
        }
        session.interface.previewer.defer = true;
        session.run_frame(Some(1), &picker.core.engine, deadline());
        if !matches!(stage, Stage::Active | Stage::Published) {
            expected.push("b");
            publish(
                session
                    .interface
                    .previewer
                    .take_request("b")
                    .start()
                    .unwrap(),
            );
        }
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
        }
        if let Some(active) = active {
            assert!(!active.is_cancelled());
            publish(active);
        }
        session.run_frame(Some(1), &picker.core.engine, deadline());
        assert_eq!(
            session.contents(),
            ["b"],
            "stage={stage:?}, detour={detour}"
        );
        assert_eq!(session.interface.previewer.requested, expected);
        assert!(
            !session
                .state
                .preview
                .prefetch
                .as_ref()
                .unwrap()
                .cache
                .contains(&1)
        );
    }
}

#[test]
fn refresh_before_hovering_invalidates_prefetched_results_and_requests() {
    for stage in std::iter::once(None).chain(Stage::ALL.map(Some)) {
        let picker = picker(["a", "b"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = stage.is_some();
        session.run_frame(Some(0), &picker.core.engine, deadline());
        let (queued, active) = stage.map_or((None, None), |stage| {
            stage.prepare(session.interface.previewer.take_request("b"))
        });

        session.handle_at(PreviewEvent::Refresh, Some(1));
        assert_eq!(session.interface.previewer.requested, ["a"]);
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
            assert!(queued.start().is_err());
        }
        if let Some(active) = active {
            assert!(active.is_cancelled());
            let mut buffer = Buffer::new();
            buffer.push_str("stale");
            assert!(active.publish(&mut buffer.into_pane()).is_err());
        }

        session.interface.previewer.lines = 2;
        assert!(session.run_frame(Some(1), &picker.core.engine, deadline()));
        assert_eq!(session.contents(), ["b", "b"], "stage={stage:?}");
        assert!(!session.run_frame(Some(1), &picker.core.engine, deadline()));
        assert_eq!(session.interface.previewer.requested, ["a", "b"]);
    }
}

#[test]
fn declining_prefetch_preserves_queued_foreground_requests() {
    let picker = picker(["a", "b"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        prefetch_limit: 4,
        ..TestPreviewer::default()
    });
    session.run_frame(Some(0), &picker.core.engine, deadline());
    let queued = session.interface.previewer.take_request("a");

    session.interface.previewer.defer = false;
    session.run_frame(Some(1), &picker.core.engine, deadline());
    assert!(!queued.is_cancelled());

    publish(queued.start().unwrap());
    session.run_frame(Some(0), &picker.core.engine, deadline());
    assert_eq!(session.interface.previewer.requested, ["a", "b"]);
    assert_eq!(session.contents(), ["a"]);
}

#[test]
fn prefetch_preserves_main_cache_recency_and_scroll() {
    let picker = picker(["a", "b", "c", "d"]);
    let mut session = TestSession::new(
        NonZero::new(3),
        TestPreviewer {
            prefetch_limit: 1,
            prefetch_enabled: true,
            lines: 20,
            ..TestPreviewer::default()
        },
    );
    session.resize(Area {
        width: 12,
        height: 5,
        ..Area::default()
    });
    for rank in [1, 2, 0] {
        session.run_frame(Some(rank), &picker.core.engine, Instant::now());
        if rank == 1 {
            session.handle_at(PreviewEvent::Down(7), Some(1));
        }
    }
    session.run_frame(Some(0), &picker.core.engine, deadline());
    assert!(session.interface.previewer.prefetched.is_empty());
    let cached = session.state.preview.cache.peek(&1).unwrap();
    assert_eq!(cached.pane().unwrap().offset().1, 7);
    for rank in [3, 2, 1] {
        session.run_frame(Some(rank), &picker.core.engine, Instant::now());
    }
    assert_eq!(
        session.interface.previewer.requested,
        ["b", "c", "a", "d", "b"]
    );
    assert_eq!(
        session
            .state
            .preview
            .cached()
            .unwrap()
            .pane()
            .unwrap()
            .offset(),
        (0, 0)
    );
}

#[test]
fn dropped_prefetches_retry_after_navigation() {
    for stage in [Stage::DroppedQueued, Stage::DroppedActive] {
        let picker = picker(["a", "b", "c"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = true;
        session.run_frame(Some(0), &picker.core.engine, deadline());
        stage.prepare(session.interface.previewer.take_request("b"));
        for _ in 0..3 {
            session.run_frame(Some(0), &picker.core.engine, deadline());
        }
        assert_eq!(session.interface.previewer.prefetched, ["b"]);
        session.run_frame(Some(2), &picker.core.engine, Instant::now());
        session.run_frame(Some(0), &picker.core.engine, deadline());
        assert_eq!(session.interface.previewer.prefetched, ["b", "b"]);
        publish(
            session
                .interface
                .previewer
                .take_request("b")
                .start()
                .unwrap(),
        );
        session.run_frame(Some(0), &picker.core.engine, deadline());
        session.run_frame(Some(1), &picker.core.engine, deadline());
        assert_eq!(session.interface.previewer.requested, ["a", "c"]);
        assert_eq!(session.contents(), ["b"]);
    }
}

#[test]
fn rejected_prefetches_retry_after_match_changes() {
    let mut picker = picker(["a", "b", "c", "d"]);
    let mut session = pane(3);
    session.interface.previewer.prefetch_enabled = false;
    for _ in 0..4 {
        session.run_frame(Some(0), &picker.core.engine, deadline());
    }
    assert_eq!(session.interface.previewer.prefetched, ["b", "c", "d"]);
    assert!(
        session
            .state
            .preview
            .prefetch
            .as_ref()
            .unwrap()
            .cache
            .is_empty()
    );
    session.interface.previewer.prefetch_enabled = true;
    for _ in 0..3 {
        session.run_frame(Some(0), &picker.core.engine, deadline());
    }
    assert_eq!(session.interface.previewer.prefetched, ["b", "c", "d"]);
    picker.push_batch(["e"]);
    settle(&mut picker);
    let cx = Context::new(&picker.core.engine)
        .with_rank(Some(0))
        .with_preview_enabled(true);
    assert!(!session.update(&cx, true, deadline()));
    for _ in 0..3 {
        session.run_frame(Some(0), &picker.core.engine, deadline());
    }
    assert_eq!(
        session.interface.previewer.prefetched,
        ["b", "c", "d", "b", "c", "d"]
    );
    for (rank, item) in [(1, "b"), (2, "c"), (3, "d")] {
        session.run_frame(Some(rank), &picker.core.engine, deadline());
        assert_eq!(session.contents(), [item]);
    }
    assert_eq!(session.interface.previewer.requested, ["a"]);
}

#[test]
fn rejected_prefetch_preserves_cached_results_and_active_work() {
    let picker = picker(["a", "b", "c", "d", "e", "f"]);
    for deferred in [false, true] {
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = deferred;
        let mut active = Vec::new();
        for (rank, item) in [(0, "b"), (2, "d")] {
            session.run_frame(Some(rank), &picker.core.engine, deadline());
            if deferred {
                active.push(
                    session
                        .interface
                        .previewer
                        .take_request(item)
                        .start()
                        .unwrap(),
                );
            }
        }

        session.interface.previewer.prefetch_enabled = false;
        for _ in 0..3 {
            session.run_frame(Some(4), &picker.core.engine, deadline());
        }
        assert_eq!(session.interface.previewer.prefetched, ["b", "d", "f"]);
        for request in active {
            assert!(!request.is_cancelled());
            publish(request);
        }
        for (rank, item) in [(1, "b"), (3, "d")] {
            session.run_frame(Some(rank), &picker.core.engine, Instant::now());
            assert_eq!(session.contents(), [item]);
        }
        assert_eq!(session.interface.previewer.requested, ["a", "c", "e"]);
    }
}

#[test]
fn hidden_match_changes_retain_retries_for_an_unchanged_neighbour_window() {
    for stage in [None, Some(Stage::DroppedQueued), Some(Stage::DroppedActive)] {
        let mut picker = picker(["a", "b", "c"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_enabled = stage.is_some();
        session.interface.previewer.prefetch_defer = stage.is_some();
        session.run_frame(Some(0), &picker.core.engine, deadline());
        if let Some(stage) = stage {
            stage.prepare(session.interface.previewer.take_request("b"));
        }
        for _ in 0..2 {
            session.run_frame(Some(0), &picker.core.engine, deadline());
        }
        assert_eq!(session.interface.previewer.prefetched, ["b"]);

        picker.push_batch(["d"]);
        settle(&mut picker);
        session.hide();
        let hidden = Context::new(&picker.core.engine).with_rank(Some(0));
        for changed in [true, false, false] {
            assert!(!session.update(&hidden, changed, deadline()));
            session.prefetch(&hidden, deadline());
        }
        assert_eq!(session.interface.previewer.requested, ["a"]);
        assert_eq!(session.interface.previewer.prefetched, ["b"]);

        session.interface.previewer.prefetch_enabled = true;
        session.interface.previewer.prefetch_defer = false;
        for _ in 0..3 {
            assert!(!session.run_frame(Some(0), &picker.core.engine, deadline()));
        }
        assert_eq!(
            session.interface.previewer.prefetched,
            ["b", "b"],
            "stage={stage:?}"
        );
        assert_eq!(session.interface.previewer.requested, ["a"]);
        session.run_frame(Some(1), &picker.core.engine, deadline());
        assert_eq!(session.contents(), ["b"]);
        assert_eq!(session.interface.previewer.requested, ["a"]);
    }
}

#[test]
fn hovered_prefetch_is_promoted_before_queued_work_is_cancelled() {
    struct Previewer {
        inner: TestPreviewer,
        active: Option<crate::preview::request::ActiveRequest<&'static str>>,
    }

    impl Preview<&'static str> for Previewer {
        fn prefetch_limit(&self) -> usize {
            1
        }

        fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
            self.inner.preview(request, timeout)
        }

        fn prefetch(
            &mut self,
            request: Request<'_, &'static str>,
            timeout: Duration,
        ) -> Result<Response, Buffer> {
            self.inner.prefetch(request, timeout)
        }

        fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
            self.inner.hover_changed(item);
            if item.is_some_and(|item| *item.value() == "beta") {
                self.active = Some(self.inner.take_request("beta").start().unwrap());
            }
        }
    }

    for matches_changed in [false, true] {
        let mut picker = picker(["alpha", "beta", "gamma"]);
        let mut session = preview_pane(Previewer {
            inner: TestPreviewer {
                prefetch_enabled: true,
                prefetch_defer: true,
                ..TestPreviewer::default()
            },
            active: None,
        });
        session.run_frame(Some(0), &picker.core.engine, deadline());
        let rank = if matches_changed {
            picker.update_query("beta");
            settle(&mut picker);
            0
        } else {
            1
        };
        let cx = Context::new(&picker.core.engine)
            .with_rank(Some(rank))
            .with_preview_enabled(true);
        assert!(session.update(&cx, matches_changed, deadline()));
        let active = session.interface.previewer.active.take().unwrap();
        assert!(!active.is_cancelled());
        publish(active);
        assert!(session.update(&cx, false, deadline()));
        assert_eq!(session.contents(), ["beta"]);
        assert_eq!(session.interface.previewer.inner.requested, ["alpha"]);
        assert_eq!(session.interface.previewer.inner.prefetched, ["beta"]);
    }
}

#[test]
fn immediately_dropped_prefetches_are_accepted_at_most_once_per_update() {
    let picker = picker(["a", "b", "c", "d"]);
    let mut session = pane(3);
    session.interface.previewer.prefetch_defer = true;
    session.interface.previewer.drop_requests = true;
    for count in 1..=8 {
        session.run_frame(Some(0), &picker.core.engine, deadline());
        assert_eq!(session.interface.previewer.prefetched.len(), count.min(3));
    }
    assert_eq!(session.interface.previewer.prefetched, ["b", "c", "d"]);
}

#[test]
fn cached_and_rejected_neighbours_consume_the_per_frame_check_budget() {
    let checks = PREFETCH_CHECKS_PER_FRAME;
    let picker = picker(std::iter::repeat_n("item", checks + 2));
    for (cached, counts) in [(true, [0, 1, 1]), (false, [checks, checks + 1, checks + 1])] {
        let mut session = pane(usize::MAX);
        session.interface.previewer.prefetch_enabled = cached;
        if cached {
            for idx in 1..=checks as u32 {
                session.run_frame(Some(idx), &picker.core.engine, Instant::now());
            }
        }
        for expected in counts {
            session.run_frame(Some(0), &picker.core.engine, deadline());
            assert_eq!(
                session.interface.previewer.prefetched.len(),
                expected,
                "cached={cached}"
            );
        }
        assert_eq!(
            session.state.preview.prefetch.as_ref().unwrap().cache.len(),
            usize::from(cached)
        );
    }
}

#[test]
fn neighbour_cursor_visits_each_candidate_once_in_distance_order() {
    for total in 1_u32..=12 {
        for rank in 0..total {
            for count in [1, 2, 4, usize::MAX] {
                let mut session = pane(count);
                let prefetch = session.state.preview.prefetch.as_mut().unwrap();
                let mut expected: Vec<_> = (0..total).filter(|&idx| idx != rank).collect();
                expected.sort_unstable_by_key(|&idx| (idx.abs_diff(rank), idx < rank));
                expected.truncate(count);
                for candidate in expected {
                    assert_eq!(prefetch.next(rank, total), Some(candidate));
                }
                for _ in 0..3 {
                    assert_eq!(prefetch.next(rank, total), None);
                }
            }
        }
    }
}

#[test]
fn neighbour_cursor_handles_maximum_rank_and_count() {
    let last = u32::MAX - 1;
    let middle = last / 2;
    for (rank, expected) in [
        (0, [1, 2, 3, 4]),
        (1, [2, 0, 3, 4]),
        (middle, [middle + 1, middle - 1, middle + 2, middle - 2]),
        (last - 1, [last, last - 2, last - 3, last - 4]),
        (last, [last - 1, last - 2, last - 3, last - 4]),
    ] {
        let mut session = pane(usize::MAX);
        let prefetch = session.state.preview.prefetch.as_mut().unwrap();
        for candidate in expected {
            assert_eq!(prefetch.next(rank, u32::MAX), Some(candidate));
        }
        prefetch.next = last - 1;
        let farthest = if rank < middle { last } else { 0 };
        assert_eq!(prefetch.next(rank, u32::MAX), Some(farthest));
        assert_eq!(prefetch.next(rank, u32::MAX), None);
    }
}

#[test]
fn declined_buffers_are_reused_through_borrowed_previewers() {
    let picker = picker(["a", "b", "c", "d", "e"]);
    let mut previewer = TestPreviewer {
        prefetch_limit: 4,
        ..TestPreviewer::default()
    };
    let mut session = preview_pane(&mut previewer);
    session.run_frame(Some(0), &picker.core.engine, Instant::now());
    session.state.preview.spare.push_str("reusable buffer");
    let pointer = session
        .state
        .preview
        .spare
        .line(0)
        .unwrap()
        .as_str()
        .as_ptr();
    for _ in 0..2 {
        session.run_frame(Some(0), &picker.core.engine, deadline());
    }
    assert!(
        session
            .state
            .preview
            .prefetch
            .as_ref()
            .unwrap()
            .cache
            .is_empty()
    );
    drop(session);
    assert!(!previewer.prefetch_allocations.is_empty());
    assert!(
        previewer
            .prefetch_allocations
            .iter()
            .all(|&allocation| allocation == pointer)
    );
}

#[test]
fn default_prefetch_returns_the_original_buffer_without_rendering() {
    let picker = picker(["a"]);
    let mut buffer = Buffer::new();
    buffer.push_str("reusable buffer");
    let pointer = buffer.line(0).unwrap().as_str().as_ptr();
    buffer.clear();
    let mut previewer = SyncPreviewer(|_: &&str, _: &mut Pane| panic!("unexpected preview"));
    let request = Request {
        buffer,
        epoch: 0,
        snapshot: picker.core.engine.snapshot(),
        id: picker.core.engine.item_id(0),
    };
    let Err(buffer) = previewer.prefetch(request, Duration::ZERO) else {
        panic!("default prefetch accepted the request");
    };
    assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), pointer);
    let session = preview_pane(&mut previewer);
    assert!(session.state.preview.prefetch.is_none());
}

#[test]
fn filtering_replaces_neighbours_without_regenerating_the_current_preview() {
    for deferred in [false, true] {
        let mut picker = picker(["alpha", "beta", "alpine", "alphabet"]);
        let mut session = pane(1);
        session.interface.previewer.prefetch_defer = deferred;
        session.run_frame(Some(0), &picker.core.engine, deadline());
        let outdated = deferred.then(|| session.interface.previewer.take_request("beta"));
        picker.update_query("al");
        settle(&mut picker);
        let cx = Context::new(&picker.core.engine)
            .with_rank(Some(0))
            .with_preview_enabled(true);
        assert!(!session.update(&cx, true, deadline()));
        session.run_frame(Some(0), &picker.core.engine, deadline());
        if let Some(outdated) = outdated {
            assert!(outdated.is_cancelled());
            publish(
                session
                    .interface
                    .previewer
                    .take_request("alpine")
                    .start()
                    .unwrap(),
            );
        }
        assert_eq!(session.interface.previewer.prefetched, ["beta", "alpine"]);
        assert_eq!(session.interface.previewer.hovered, [Some("alpha")]);
        session.run_frame(Some(1), &picker.core.engine, Instant::now());
        assert_eq!(session.contents(), ["alpine"]);
        assert_eq!(session.interface.previewer.requested, ["alpha"]);
    }
}

#[test]
fn prefetch_eviction_and_hovered_item_promotion_reuse_buffers() {
    let picker = picker(["a", "b", "c", "d", "e", "f", "g", "h"]);
    let mut session = TestSession::new(
        NonZero::new(1),
        TestPreviewer {
            prefetch_limit: 1,
            prefetch_enabled: true,
            ..TestPreviewer::default()
        },
    );
    session.run_frame(Some(0), &picker.core.engine, deadline());
    let cache = &session.state.preview.prefetch.as_ref().unwrap().cache;
    let first = allocation(cache.peek(&1).unwrap());
    for rank in [3, 6] {
        session.run_frame(Some(rank), &picker.core.engine, deadline());
    }
    let cache = &session.state.preview.prefetch.as_ref().unwrap().cache;
    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&1));
    assert_eq!(
        session
            .state
            .preview
            .spare
            .line(0)
            .unwrap()
            .as_str()
            .as_ptr(),
        first
    );
    let promoted = allocation(cache.peek(&7).unwrap());
    session.run_frame(Some(7), &picker.core.engine, deadline());
    assert_eq!(
        allocation(session.state.preview.cache.peek(&7).unwrap()),
        promoted
    );
    assert!(
        !session
            .state
            .preview
            .prefetch
            .as_ref()
            .unwrap()
            .cache
            .contains(&7)
    );
    session.run_frame(Some(4), &picker.core.engine, deadline());
    assert_eq!(session.interface.previewer.requested, ["a", "d", "g"]);
    assert_eq!(
        session.interface.previewer.prefetched,
        ["b", "e", "h", "g", "f"]
    );
    assert_eq!(
        session.interface.previewer.prefetch_allocations.last(),
        Some(&promoted)
    );
    assert_eq!(session.state.preview.cache.len(), 1);
    let cached = session.state.preview.cache.peek(&4).unwrap();
    assert_eq!(cached.pane().unwrap().offset(), (0, 0));
}

#[test]
fn eviction_cancels_active_prefetches() {
    let picker = picker(["a", "b", "c", "d", "e", "f"]);
    let mut session = pane(1);
    session.interface.previewer.prefetch_defer = true;
    session.run_frame(Some(0), &picker.core.engine, deadline());
    let first = session
        .interface
        .previewer
        .take_request("b")
        .start()
        .unwrap();
    session.run_frame(Some(2), &picker.core.engine, deadline());
    let second = session
        .interface
        .previewer
        .take_request("d")
        .start()
        .unwrap();
    session.run_frame(Some(4), &picker.core.engine, deadline());
    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
    let mut buffer = Buffer::new();
    buffer.push_str("cancelled result");
    let mut pane = buffer.into_pane();
    assert!(first.publish(&mut pane).is_err());
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "cancelled result");
}
