use super::*;
use crate::preview::{
    DEFAULT_CACHE_SIZE, Pane, SyncPreviewer,
    request::{QueuedRequest, Response},
};
use crate::rect::Area;

mod prefetch;
pub(crate) mod support;
use support::*;

#[test]
fn synchronous_generation_preserves_initial_presentation_and_reuses_storage_on_refresh() {
    fn fill(pane: &mut Pane, item: &str) {
        assert_eq!(pane.offset(), (0, 0));
        assert!(!pane.line_numbers());
        assert_eq!(pane.buffer().lines().len(), 1);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
        assert!(!pane.is_err());
        pane.push_text(item);
        pane.set_err(true);
    }

    struct ReadyPreview;

    impl Preview<&'static str> for ReadyPreview {
        fn preview(&mut self, request: Request<'_, &'static str>, _: Duration) -> Response {
            let item = request.item();
            let mut pane: Pane = request.ready();
            fill(&mut pane, item);
            Response::Ready(pane.with_offset(5, 17).with_line_numbers(true))
        }
    }

    fn check(previewer: impl Preview<&'static str>) {
        let picker = picker(["alpha"]);
        let mut session = preview_pane(previewer);
        session.resize(Area {
            width: 12,
            height: 5,
            ..Area::default()
        });
        session
            .state
            .preview
            .spare
            .push_str("reusable request storage");
        let allocation = session
            .state
            .preview
            .spare
            .line(0)
            .unwrap()
            .as_str()
            .as_ptr();
        for _ in 0..2 {
            assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
            assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
            let Content::Ready(pane) = session.state.preview.content() else {
                panic!("expected synchronous preview");
            };
            assert_eq!(pane.offset(), (5, 17));
            assert!(pane.line_numbers());
            assert!(pane.is_err());
            assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
            assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
            session.handle_at(PreviewEvent::AlignTop, Some(id_at(&picker, 0)));
            session.handle_at(PreviewEvent::SetLineNumbers(false), Some(id_at(&picker, 0)));
            session.handle_at(PreviewEvent::Refresh, Some(id_at(&picker, 0)));
        }
    }

    check(ReadyPreview);
    check(SyncPreviewer(|item: &&str, pane: &mut Pane| {
        fill(pane, item);
        pane.set_offset(5, 17);
        pane.set_line_numbers(true);
    }));
}

#[test]
fn preview_events_use_context_identity_and_applied_geometry() {
    for reverse in [false, true] {
        let mut driver = crate::DriverOptions::new()
            .reverse_items(reverse)
            .sort_results(false)
            .driver(crate::render::StrRenderer);
        driver.push_batch(["alpha", "beta"]);
        let matches = driver.filter().unwrap();
        let alpha = matches.get(usize::from(reverse)).unwrap().id();
        let beta = matches.get(usize::from(!reverse)).unwrap().id();
        driver.update_query("beta");
        driver.filter().unwrap();
        let mut session = preview_pane(TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        });
        session.resize(Area {
            width: 22,
            height: 10,
            ..Area::default()
        });
        let cx = Context::new(driver.matches())
            .with_rank(Some(0))
            .with_preview_enabled(true);
        assert_eq!(cx.hovered().unwrap().id(), beta);
        session.update(&cx, true, Instant::now());
        session.handle(PreviewEvent::PageDown(1), &cx);
        let cached = session.state.preview.cache.peek(&beta).unwrap();
        assert_eq!(cached.pane().unwrap().offset(), (0, 8));
        session.state.preview.resize(Some(Dimensions {
            width: 4,
            height: 3,
        }));
        session.handle(PreviewEvent::PageDown(1), &cx);
        let cached = session.state.preview.cache.peek(&beta).unwrap();
        assert_eq!(cached.pane().unwrap().offset(), (0, 11));
        assert!(!session.state.preview.cache.contains(&alpha));
        assert_eq!(session.interface.previewer.requested, ["beta"]);
        assert_eq!(session.interface.previewer.ids, [beta]);
    }
}

#[test]
fn cached_identity_preserves_scroll_and_line_numbers_until_restart() {
    let mut picker = picker(["alpha", "beta", "alpha"]);
    let beta = id_at(&picker, 1);
    let mut previewer = TestPreviewer::default();
    let mut session = preview_pane(&mut previewer);
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    for hovered in [None, Some(beta)] {
        assert!(!session.handle_at(PreviewEvent::ToggleLineNumbers, hovered));
        assert!(!session.handle_at(PreviewEvent::SetLineNumbers(true), hovered));
    }
    assert_eq!(session.state.preview.cache.len(), 1);
    let cached = session
        .state
        .preview
        .cache
        .get_mut(&id_at(&picker, 0))
        .unwrap();
    assert!(!cached.pane().unwrap().line_numbers());
    cached.pane_mut().unwrap().set_offset(12, 7);
    session.handle_at(PreviewEvent::ToggleLineNumbers, Some(id_at(&picker, 0)));

    picker.update_query("beta");
    settle(&mut picker);
    assert_eq!(picker.matches().get(0).unwrap().id(), beta);
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert_eq!(session.contents(), ["beta"]);
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());

    picker.update_query("");
    settle(&mut picker);
    for (rank, changed) in [(0, true), (0, false), (2, true), (0, true), (0, false)] {
        assert_eq!(
            session.update_at(Some(rank), picker.matches(), Instant::now()),
            changed
        );
        let pane = session.state.preview.cached().unwrap().pane().unwrap();
        assert_eq!(pane.offset(), if rank == 0 { (12, 7) } else { (0, 0) });
        assert_eq!(pane.line_numbers(), rank == 0);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
    }
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "beta", "alpha"]
    );
    session.hide();
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    session.restart();
    picker.restart();
    picker.push_batch(["alpha"]);
    settle(&mut picker);
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    session.end();
    drop(session);

    let [first, beta, duplicate, restarted] = previewer.ids[..] else {
        unreachable!();
    };
    assert_ne!(first, beta);
    assert_ne!(first, duplicate);
    assert_ne!(first, restarted);
    assert_ne!(duplicate, restarted);
    assert_eq!(
        previewer.hovered_ids,
        [
            Some(first),
            Some(beta),
            Some(first),
            Some(duplicate),
            Some(first),
            None,
            Some(first),
            None,
            Some(restarted),
            None,
        ]
    );
}

#[test]
fn cleared_hovered_item_is_not_reported_again_on_restart_or_end() {
    for hidden in [false, true] {
        for restart in [false, true] {
            let picker = picker(["alpha"]);
            let mut previewer = TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            };
            let mut session = preview_pane(&mut previewer);
            session.update_at(Some(0), picker.matches(), Instant::now());
            if hidden {
                session.hide();
            } else {
                session.update_at(None, picker.matches(), Instant::now());
            }
            if restart {
                session.restart();
            }
            session.end();
            drop(session);

            assert_eq!(
                previewer.hovered,
                [Some("alpha"), None],
                "hidden={hidden}, restart={restart}"
            );
            assert_eq!(previewer.cancelled_on_clear, [false]);
            assert!(previewer.queued[0].is_cancelled());
            assert_eq!(previewer.restarts, usize::from(restart));
        }
    }
}

#[test]
fn refresh_and_eviction_reuse_buffers_and_reset_display_settings() {
    let picker = picker(["alpha", "beta"]);
    let mut session = TestSession::new(
        NonZero::new(1),
        TestPreviewer {
            lines: 20,
            ..TestPreviewer::default()
        },
    );
    session.resize(Area {
        width: 12,
        height: 5,
        ..Area::default()
    });
    session.update_at(Some(0), picker.matches(), Instant::now());
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    let allocation = pane.buffer().line(0).unwrap().as_str().as_ptr();

    session.interface.previewer.lines = 4;
    for (step, (rank, refresh)) in [(0, true), (1, false), (0, true), (1, false)]
        .into_iter()
        .enumerate()
    {
        let previous = session.state.preview.last_item.unwrap();
        let cached = session.state.preview.cache.get_mut(&previous).unwrap();
        let pane = cached.pane_mut().unwrap();
        pane.set_offset(7, 8);
        pane.buffer_mut().push_text("\nprevious contents\n");
        pane.set_err(true);
        pane.set_line_numbers(true);
        if refresh {
            for _ in 0..2 {
                session.handle_at(PreviewEvent::Refresh, session.state.preview.last_item);
            }
        }
        assert_eq!(session.interface.previewer.requested.len(), step + 1);
        assert!(session.update_at(Some(rank), picker.matches(), Instant::now()));
        assert!(!session.update_at(Some(rank), picker.matches(), Instant::now()));
        assert_eq!(session.state.preview.cache.len(), 1);
        assert_eq!(
            session.interface.previewer.buffer_allocations[step + 1],
            allocation
        );
        let pane = session.state.preview.cached().unwrap().pane().unwrap();
        assert_eq!(pane.offset(), (0, 0));
        assert_eq!(pane.buffer().lines().len(), 4);
        assert!(!pane.is_err());
        assert!(!pane.line_numbers());
    }
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "alpha", "beta", "alpha", "beta"]
    );
    assert_eq!(
        session.interface.previewer.hovered,
        [Some("alpha"), Some("beta"), Some("alpha"), Some("beta")]
    );
    assert_eq!(
        session.interface.previewer.ids[0],
        session.interface.previewer.ids[1]
    );
}

#[test]
fn refresh_only_invalidates_the_target_without_resubmitting_queued_work() {
    let picker = picker(["alpha", "beta", "gamma"]);
    let mut session = preview_pane(TestPreviewer::default());
    for rank in [0, 1] {
        session.update_at(Some(rank), picker.matches(), Instant::now());
    }
    for hovered in [None, Some(id_at(&picker, 2))] {
        assert!(!session.handle_at(PreviewEvent::Refresh, hovered));
    }
    assert_eq!(session.state.preview.cache.len(), 2);
    assert_eq!(session.interface.previewer.requested, ["alpha", "beta"]);
    let cached = session
        .state
        .preview
        .cache
        .peek_mut(&id_at(&picker, 1))
        .unwrap();
    cached.pane_mut().unwrap().set_offset(0, 7);
    session.handle_at(PreviewEvent::Refresh, Some(id_at(&picker, 0)));
    assert!(!session.update_at(Some(1), picker.matches(), Instant::now()));
    assert_eq!(session.interface.previewer.requested, ["alpha", "beta"]);
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.offset(), (0, 7));
    session.interface.previewer.defer = true;
    assert!(session.interface.previewer.queued.is_empty());
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(session.update_at(Some(1), picker.matches(), Instant::now()));
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.offset(), (0, 7));
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "beta", "alpha"]
    );
    assert_eq!(session.interface.previewer.queued.len(), 1);
    assert!(!session.interface.previewer.queued[0].is_cancelled());
}

#[test]
fn cache_evicts_the_least_recently_visited_item() {
    let mut session = TestSession::new(NonZero::new(3), TestPreviewer::default());
    let capacity = session.state.preview.cache.cap().get();
    assert_eq!(capacity, 3);
    let picker = picker(std::iter::repeat_n("item", capacity + 1));
    for rank in 0..capacity as u32 {
        session.update_at(Some(rank), picker.matches(), Instant::now());
    }
    session.update_at(Some(0), picker.matches(), Instant::now());
    session.update_at(Some(capacity as u32), picker.matches(), Instant::now());

    assert_eq!(session.state.preview.cache.len(), capacity);
    assert!(session.state.preview.cache.contains(&id_at(&picker, 0)));
    assert!(!session.state.preview.cache.contains(&id_at(&picker, 1)));
    assert_eq!(session.interface.previewer.requested.len(), capacity + 1);

    session.update_at(Some(1), picker.matches(), Instant::now());
    assert_eq!(session.interface.previewer.requested.len(), capacity + 2);
    let cached = session
        .state
        .preview
        .cache
        .peek(&id_at(&picker, 1))
        .unwrap();
    assert_eq!(cached.pane().unwrap().offset(), (0, 0));
}

#[test]
fn unbounded_cache_retains_previews_and_scroll_state() {
    let mut session = TestSession::new(None, TestPreviewer::default());
    let item_count = DEFAULT_CACHE_SIZE.unwrap().get() + 1;
    let picker = picker(std::iter::repeat_n("item", item_count));
    for rank in 0..item_count as u32 {
        session.update_at(Some(rank), picker.matches(), Instant::now());
        let cached = session
            .state
            .preview
            .cache
            .get_mut(&id_at(&picker, rank))
            .unwrap();
        cached.pane_mut().unwrap().set_offset(0, rank as usize + 1);
    }
    assert_eq!(session.state.preview.cache.len(), item_count);

    for rank in 0..item_count as u32 {
        session.update_at(Some(rank), picker.matches(), Instant::now());
        assert_eq!(
            session
                .state
                .preview
                .cached()
                .unwrap()
                .pane()
                .unwrap()
                .offset()
                .1,
            rank as usize + 1
        );
    }
    assert_eq!(session.interface.previewer.requested.len(), item_count);
}

#[test]
fn refresh_and_eviction_cancel_requests_and_reuse_returned_buffers() {
    for (stage, evict) in Stage::ALL
        .into_iter()
        .flat_map(|stage| [(stage, false), (stage, true)])
    {
        let mut session = TestSession::new(NonZero::new(1), TestPreviewer::default());
        let picker = picker(["alpha", "beta"]);
        session.state.preview.spare.push_str("reusable buffer");
        let mut allocation = session
            .state
            .preview
            .spare
            .line(0)
            .unwrap()
            .as_str()
            .as_ptr();
        session.interface.previewer.defer = true;
        session.update_at(Some(0), picker.matches(), Instant::now());
        let request = session.interface.previewer.take_request("alpha");
        let (queued, active) = if matches!(stage, Stage::Published) {
            let mut pane = Pane::new();
            pane.buffer_mut().push_text("completed alpha\n");
            allocation = pane.buffer().line(0).unwrap().as_str().as_ptr();
            request.start().unwrap().publish(&mut pane).unwrap();
            (None, None)
        } else {
            stage.prepare(request)
        };

        session.interface.previewer.defer = false;
        session.interface.previewer.lines = 2;
        let (rank, item) = if evict {
            assert!(session.update_at(Some(1), picker.matches(), Instant::now()));
            (1, "beta")
        } else {
            session.handle_at(PreviewEvent::Refresh, Some(id_at(&picker, 0)));
            assert_eq!(session.interface.previewer.requested, ["alpha"]);
            (0, "alpha")
        };
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
            assert!(queued.start().is_err());
        }
        if let Some(active) = active {
            assert!(active.is_cancelled());
        }
        if !evict {
            assert!(session.update_at(Some(rank), picker.matches(), Instant::now()));
        }
        assert!(!session.update_at(Some(rank), picker.matches(), Instant::now()));
        assert_eq!(session.interface.previewer.requested, ["alpha", item]);
        assert_eq!(
            session.interface.previewer.buffer_allocations[1], allocation,
            "stage={stage:?}, evict={evict}"
        );
        assert_eq!(session.contents(), [item; 2]);
    }
}

#[test]
fn queued_promotion_and_dropped_requests_reuse_buffers_once_per_update() {
    let picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    session.state.preview.spare.push_str("previous contents");
    let allocation = session
        .state
        .preview
        .spare
        .line(0)
        .unwrap()
        .as_str()
        .as_ptr();
    for rank in [0, 0, 1, 0, 0, 0] {
        session.update_at(Some(rank), picker.matches(), Instant::now());
    }
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "beta", "alpha"]
    );
    assert_eq!(
        session.interface.previewer.hovered,
        [Some("alpha"), Some("beta"), Some("alpha")]
    );
    assert!(session.interface.previewer.queued[0].is_cancelled());
    assert!(!session.interface.previewer.queued[1].is_cancelled());
    assert!(!session.interface.previewer.queued[2].is_cancelled());
    assert_eq!(
        session.interface.previewer.buffer_allocations[2],
        allocation
    );
    for (step, stage) in [Stage::DroppedQueued, Stage::DroppedActive]
        .into_iter()
        .enumerate()
    {
        let Some(RequestState::Pending(pending)) = &session.state.preview.cached().unwrap().state
        else {
            panic!("expected pending preview");
        };
        assert_eq!(pending.epoch, session.state.preview.epoch);
        stage.prepare(session.interface.previewer.take_request("alpha"));
        assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
        assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
        assert_eq!(session.interface.previewer.requested.len(), step + 4);
        assert_eq!(
            session.interface.previewer.buffer_allocations[step + 3],
            allocation
        );
        assert_eq!(
            session.interface.previewer.ids[step + 3],
            session.interface.previewer.ids[0]
        );
    }
    session.interface.previewer.defer = false;
    drop(session.interface.previewer.take_request("alpha"));
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
    assert_eq!(session.contents(), ["alpha"]);
    assert!(!pane.is_err());
    assert!(!pane.line_numbers());
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "beta", "alpha", "alpha", "alpha", "alpha"]
    );
    assert_eq!(
        session.interface.previewer.hovered,
        [Some("alpha"), Some("beta"), Some("alpha")]
    );
}

#[test]
fn active_previews_survive_revisits_and_complete_once() {
    let picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    session.update_at(Some(0), picker.matches(), Instant::now());
    let active = session
        .interface
        .previewer
        .queued
        .pop()
        .unwrap()
        .start()
        .unwrap();
    session.hide();
    session.update_at(Some(0), picker.matches(), Instant::now());
    session.update_at(Some(1), picker.matches(), Instant::now());
    let queued = session.interface.previewer.take_request("beta");
    for rank in [None, None, Some(0), Some(0)] {
        session.update_at(rank, picker.matches(), Instant::now());
    }
    assert!(!active.is_cancelled());
    assert_eq!(session.interface.previewer.requested, ["alpha", "beta"]);
    let mut pane = Pane::new();
    pane.buffer_mut().push_str("completed alpha");
    pane.set_err(true);
    assert!(active.publish(&mut pane).is_ok());
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    let pane = session.state.preview.cached().unwrap().pane().unwrap();
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "completed alpha");
    assert!(pane.is_err());
    assert_eq!(
        session.interface.previewer.hovered,
        [
            Some("alpha"),
            None,
            Some("alpha"),
            Some("beta"),
            None,
            Some("alpha")
        ]
    );
    publish(queued.start().unwrap());
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(matches!(
        session
            .state
            .preview
            .cache
            .peek(&id_at(&picker, 1))
            .unwrap()
            .state,
        Some(RequestState::Pending(_))
    ));
    assert!(session.update_at(Some(1), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(1), picker.matches(), Instant::now()));
    assert_eq!(session.contents(), ["beta"]);
    assert_eq!(session.interface.previewer.requested, ["alpha", "beta"]);
}

#[test]
fn filtering_and_cleared_hovered_items_reprioritize_by_item_identity() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    for (query, changed) in [
        ("", true),
        ("alp", false),
        ("beta", true),
        ("alpha", true),
        ("missing", true),
        ("missing", false),
        ("alpha", true),
    ] {
        picker.update_query(query);
        settle(&mut picker);
        assert_eq!(update(&mut session, &mut picker), changed);
        assert_eq!(session.state.preview.cached().is_none(), query == "missing");
    }
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "beta", "alpha", "alpha"]
    );
    let ids = &session.interface.previewer.ids;
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[0], ids[2]);
    assert_eq!(ids[0], ids[3]);
    assert!(session.interface.previewer.queued[0].is_cancelled());
    assert!(!session.interface.previewer.queued[1].is_cancelled());
    assert!(session.interface.previewer.queued[2].is_cancelled());
    assert!(!session.interface.previewer.queued[3].is_cancelled());
    assert_eq!(
        session.interface.previewer.hovered,
        [
            Some("alpha"),
            Some("beta"),
            Some("alpha"),
            None,
            Some("alpha")
        ]
    );
}

#[test]
fn immediately_dropped_requests_are_submitted_at_most_once_per_update() {
    let picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        drop_requests: true,
        ..TestPreviewer::default()
    });
    for count in 1..=4 {
        assert_eq!(
            session.update_at(Some(0), picker.matches(), Instant::now()),
            count == 1
        );
        assert_eq!(session.interface.previewer.requested.len(), count);
    }
    session.interface.previewer.defer = false;
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    assert_eq!(session.interface.previewer.requested.len(), 5);
}

#[test]
fn restart_clears_preview_state_without_deferring_changes_to_update() {
    for defer in [false, true] {
        for restarts in [1, 2] {
            let mut picker = picker([]);
            let mut session = preview_pane(TestPreviewer {
                defer,
                ..TestPreviewer::default()
            });
            assert!(!update(&mut session, &mut picker));
            assert!(!update(&mut session, &mut picker));
            assert!(session.interface.previewer.hovered.is_empty());
            assert!(session.interface.previewer.requested.is_empty());
            assert!(session.state.preview.cache.is_empty());
            assert!(session.state.preview.cached().is_none());
            picker.push_batch(["alpha"]);
            settle(&mut picker);
            assert!(update(&mut session, &mut picker));

            picker.restart();
            settle(&mut picker);
            assert!(picker.matches().is_empty());
            for _ in 0..restarts {
                session.restart();
            }
            assert!(session.state.preview.cache.is_empty());
            assert!(
                session
                    .interface
                    .previewer
                    .queued
                    .iter()
                    .all(QueuedRequest::is_cancelled)
            );
            assert!(matches!(session.state.preview.content(), Content::Empty));
            assert!(!update(&mut session, &mut picker));
            assert!(!update(&mut session, &mut picker));
            assert_eq!(session.state.preview.epoch, 0);
            assert_eq!(session.interface.previewer.requested, ["alpha"]);

            session.restart();
            assert!(!update(&mut session, &mut picker));
            assert_eq!(session.interface.previewer.hovered, [Some("alpha"), None]);
        }
    }
}

#[test]
fn restart_with_a_replacement_item_and_pending_preview_reports_one_change() {
    let mut picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer::default());
    update(&mut session, &mut picker);
    let old_id = id_at(&picker, 0);

    session.restart();
    assert_eq!(session.state.preview.last_item, None);
    assert_eq!(session.state.preview.epoch, 0);
    assert!(session.state.preview.cache.is_empty());
    assert!(update(&mut session, &mut picker));
    assert_eq!(id_at(&picker, 0), old_id);
    assert_eq!(session.interface.previewer.requested, ["alpha", "alpha"]);

    session.restart();
    picker.restart();
    picker.push_batch(["beta"]);
    settle(&mut picker);
    let new_id = id_at(&picker, 0);
    assert_ne!(new_id, old_id);
    assert!(!new_id.same_generation(old_id));
    session.interface.previewer.defer = true;

    assert!(update(&mut session, &mut picker));
    assert!(!update(&mut session, &mut picker));
    assert_eq!(
        session.interface.previewer.requested,
        ["alpha", "alpha", "beta"]
    );
    assert!(!session.interface.previewer.queued[0].is_cancelled());
    assert_eq!(
        session.interface.previewer.hovered,
        [Some("alpha"), None, Some("alpha"), None, Some("beta")]
    );
}

#[test]
fn pending_previews_ignore_scrolling_even_after_publication() {
    let picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let area = Area {
        width: 22,
        height: 10,
        ..Area::default()
    };
    session.resize(area);
    session.update_at(Some(0), picker.matches(), Instant::now());
    session.handle_at(PreviewEvent::Down(1), Some(id_at(&picker, 0)));
    session.handle_at(PreviewEvent::Right(1), Some(id_at(&picker, 0)));
    let active = session
        .interface
        .previewer
        .queued
        .pop()
        .unwrap()
        .start()
        .unwrap();
    session.handle_at(PreviewEvent::PageDown(1), Some(id_at(&picker, 0)));
    let mut pane = Pane::new();
    for _ in 0..30 {
        pane.buffer_mut().push_line("abcdefghijklmnopqrstuvwxyz");
    }
    assert!(active.publish(&mut pane).is_ok());
    for event in [
        PreviewEvent::Right(usize::MAX),
        PreviewEvent::PageUp(1),
        PreviewEvent::AlignLeft,
        PreviewEvent::AlignRight,
        PreviewEvent::AlignTop,
        PreviewEvent::AlignBottom,
    ] {
        session.handle_at(event, Some(id_at(&picker, 0)));
    }
    session.resize(area);
    assert!(matches!(
        session
            .state
            .preview
            .cache
            .peek(&id_at(&picker, 0))
            .unwrap()
            .state,
        Some(RequestState::Pending(_))
    ));
    session.update_at(Some(0), picker.matches(), Instant::now());
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
fn lines_and_pages_saturate_in_order_and_preserve_columns() {
    let picker = picker(["alpha"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 23,
        ..TestPreviewer::default()
    });
    let cx = Context::new(picker.matches())
        .with_rank(Some(0))
        .with_preview_enabled(true);
    let size = Dimensions {
        width: 20,
        height: 8,
    };
    session.state.preview.resize(Some(size));
    assert!(session.update(&cx, true, Instant::now()));
    let cached = session
        .state
        .preview
        .cache
        .peek_mut(&id_at(&picker, 0))
        .unwrap();
    cached.pane_mut().unwrap().set_offset(9, 0);
    for (event, row) in [
        (PreviewEvent::Up(5), 0),
        (PreviewEvent::Down(2), 2),
        (PreviewEvent::PageDown(1), 10),
        (PreviewEvent::PageDown(usize::MAX), 15),
        (PreviewEvent::Down(usize::MAX), 15),
        (PreviewEvent::Down(5), 15),
        (PreviewEvent::Up(5), 10),
        (PreviewEvent::PageUp(1), 2),
        (PreviewEvent::Up(usize::MAX), 0),
        (PreviewEvent::Down(usize::MAX), 15),
        (PreviewEvent::PageUp(usize::MAX), 0),
        (PreviewEvent::Down(0), 0),
        (PreviewEvent::PageDown(0), 0),
        (PreviewEvent::AlignBottom, 15),
        (PreviewEvent::AlignBottom, 15),
        (PreviewEvent::AlignTop, 0),
        (PreviewEvent::AlignTop, 0),
    ] {
        let previous = session
            .state
            .preview
            .cached()
            .unwrap()
            .pane()
            .unwrap()
            .offset();
        assert_eq!(session.handle(event, &cx), previous != (9, row));
        assert_eq!(
            session
                .state
                .preview
                .cached()
                .unwrap()
                .pane()
                .unwrap()
                .offset(),
            (9, row)
        );
        assert!(!session.update(&cx, false, Instant::now()));
    }
    for lines in [1, 3, 8] {
        let mut session = preview_pane(TestPreviewer {
            lines,
            ..TestPreviewer::default()
        });
        session.state.preview.resize(Some(size));
        assert!(session.update(&cx, true, Instant::now()));
        for event in [
            PreviewEvent::Down(1),
            PreviewEvent::PageDown(usize::MAX),
            PreviewEvent::AlignTop,
            PreviewEvent::AlignBottom,
        ] {
            assert!(!session.handle(event, &cx));
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
            assert!(!session.update(&cx, false, Instant::now()));
        }
    }
}

#[test]
fn zero_sized_panes_only_suppress_movement_at_zero_width() {
    let picker = picker(["abcdefghijklmnop"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 4,
        ..TestPreviewer::default()
    });
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    for (width, height) in [(0, 1), (1, 0), (0, 0)] {
        let cx = Context::new(picker.matches())
            .with_rank(Some(0))
            .with_preview_enabled(true);
        session
            .state
            .preview
            .resize(Some(Dimensions { width, height }));
        for (event, offset) in [
            (PreviewEvent::Left(1), (6, 2)),
            (PreviewEvent::Right(1), (7, 2)),
            (PreviewEvent::Up(1), (7, 1)),
            (PreviewEvent::Down(1), (7, 3)),
            (PreviewEvent::PageUp(1), (7, 2)),
            (PreviewEvent::PageDown(1), (7, 2)),
            (PreviewEvent::AlignLeft, (0, 2)),
            (PreviewEvent::AlignRight, (0, 2)),
            (PreviewEvent::AlignTop, (7, 0)),
            (PreviewEvent::AlignBottom, (7, 4)),
        ] {
            let cached = session
                .state
                .preview
                .cache
                .peek_mut(&id_at(&picker, 0))
                .unwrap();
            cached.pane_mut().unwrap().set_offset(7, 2);
            let expected = if width == 0 { (7, 2) } else { offset };
            assert_eq!(session.handle(event, &cx), expected != (7, 2));
            assert_eq!(
                session
                    .state
                    .preview
                    .cached()
                    .unwrap()
                    .pane()
                    .unwrap()
                    .offset(),
                expected
            );
            assert!(!session.update(&cx, false, Instant::now()));
        }
    }
}

#[test]
fn scrolling_reports_changes_without_advancing_request_priority() {
    let picker = picker(["alpha", "beta"]);
    let mut session = preview_pane(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    session.update_at(Some(0), picker.matches(), Instant::now());
    let epoch = session.state.preview.epoch;
    session.resize(Area {
        column: 10,
        row: 0,
        width: 10,
        height: 10,
    });
    assert!(session.handle_at(PreviewEvent::Down(1), Some(id_at(&picker, 0))));
    assert!(!session.handle_at(PreviewEvent::Up(0), Some(id_at(&picker, 0))));
    let cached = session
        .state
        .preview
        .cache
        .peek(&id_at(&picker, 0))
        .unwrap();
    assert_eq!(cached.pane().unwrap().offset().1, 1);
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    assert_eq!(session.state.preview.epoch, epoch);
    assert_eq!(session.interface.previewer.requested, ["alpha"]);

    session.handle_at(PreviewEvent::Down(1), Some(id_at(&picker, 0)));
    session.interface.previewer.defer = true;
    assert!(session.update_at(Some(1), picker.matches(), Instant::now()));
    assert!(!session.update_at(Some(1), picker.matches(), Instant::now()));
    assert_eq!(session.state.preview.epoch, epoch + 1);
    assert_eq!(session.interface.previewer.requested, ["alpha", "beta"]);
    assert!(!session.interface.previewer.queued[0].is_cancelled());
}

#[test]
fn restart_and_end_cancel_foreground_and_prefetch_requests_before_callbacks() {
    for restart in [false, true] {
        let picker = picker(["alpha", "beta", "gamma", "delta"]);
        let mut previewer = TestPreviewer {
            defer: true,
            prefetch_limit: 4,
            prefetch_enabled: true,
            prefetch_defer: true,
            ..TestPreviewer::default()
        };
        let mut session = preview_pane(&mut previewer);
        if restart {
            session.restart();
        }
        session.update_at(Some(3), picker.matches(), Instant::now());
        session.update_at(Some(0), picker.matches(), Instant::now());
        let foreground = session
            .interface
            .previewer
            .take_request("alpha")
            .start()
            .unwrap();
        session.run_frame(Some(0), picker.matches(), deadline());
        let prefetched = session
            .interface
            .previewer
            .take_request("beta")
            .start()
            .unwrap();
        session.run_frame(Some(0), picker.matches(), deadline());
        assert_eq!(session.interface.previewer.requested, ["delta", "alpha"]);
        assert_eq!(session.interface.previewer.prefetched, ["beta", "gamma"]);
        assert!(!foreground.is_cancelled());
        assert!(!prefetched.is_cancelled());
        assert!(
            session
                .interface
                .previewer
                .queued
                .iter()
                .all(|request| !request.is_cancelled())
        );

        if restart {
            session.restart();
            assert!(session.state.preview.cache.is_empty());
            assert_eq!(session.state.preview.last_item, None);
            assert_eq!(session.state.preview.epoch, 0);
            assert!(foreground.is_cancelled());
            assert!(prefetched.is_cancelled());
            session.restart();
        }
        session.end();
        drop(session);

        assert_eq!(
            previewer.hovered,
            [Some("delta"), Some("alpha"), None],
            "restart={restart}"
        );
        assert_eq!(previewer.cancelled_on_clear, [true]);
        assert_eq!(previewer.restarts, if restart { 3 } else { 0 });
        assert_eq!(previewer.queued.len(), 2);
        for queued in previewer.queued {
            assert!(queued.is_cancelled());
            assert!(queued.start().is_err());
        }
        for active in [foreground, prefetched] {
            assert!(active.is_cancelled());
            let mut pane = Pane::new();
            pane.buffer_mut().push_str("unused result");
            assert!(active.publish(&mut pane).is_err());
            assert_eq!(pane.buffer().line(0).unwrap().as_str(), "unused result");
        }
    }
}

#[test]
fn owned_state_preserves_requests_until_end_or_drop() {
    for end in [false, true] {
        let picker = picker(["alpha", "beta"]);
        let mut previewer = TestPreviewer {
            defer: true,
            prefetch_limit: 1,
            prefetch_enabled: true,
            prefetch_defer: true,
            ..TestPreviewer::default()
        };
        let cx = Context::new(picker.matches())
            .with_rank(Some(0))
            .with_preview_enabled(true);
        let (mut state, active) = {
            let mut state = PreviewState::new(None, previewer.prefetch_limit());
            assert!(state.update(&mut previewer, &cx, true, Instant::now()));
            let active = previewer.take_request("alpha").start().unwrap();
            assert!(!state.update(&mut previewer, &cx, false, Instant::now()));
            state.update_prefetch(&mut previewer, &cx, deadline());
            (state, active)
        };

        assert!(!active.is_cancelled());
        assert_eq!(previewer.prefetched, ["beta"]);
        assert!(!previewer.queued[0].is_cancelled());
        assert!(!state.update(&mut previewer, &cx, false, Instant::now()));
        assert_eq!(previewer.requested, ["alpha"]);
        if end {
            state.end(&mut previewer);
            assert!(active.is_cancelled());
            assert!(previewer.queued[0].is_cancelled());
        }
        drop(state);

        assert!(active.is_cancelled());
        assert!(previewer.queued[0].is_cancelled());
        if end {
            assert_eq!(previewer.hovered, [Some("alpha"), None]);
            assert_eq!(previewer.cancelled_on_clear, [true]);
        } else {
            assert_eq!(previewer.hovered, [Some("alpha")]);
            assert!(previewer.cancelled_on_clear.is_empty());
        }
    }
}

#[test]
fn session_cancels_requests_before_dropping_its_previewer() {
    struct PreviewerDrop<'a> {
        queued: Option<QueuedRequest<&'static str>>,
        cancelled: &'a std::cell::Cell<bool>,
    }

    impl Preview<&'static str> for PreviewerDrop<'_> {
        fn preview(&mut self, request: Request<'_, &'static str>, _: Duration) -> Response {
            let (pending, queued) = request.defer();
            self.queued = Some(queued);
            Response::Pending(pending)
        }
    }

    impl Drop for PreviewerDrop<'_> {
        fn drop(&mut self) {
            self.cancelled.set(
                self.queued
                    .as_ref()
                    .is_some_and(QueuedRequest::is_cancelled),
            );
        }
    }

    let picker = picker(["alpha"]);
    for unwind in [false, true] {
        let cancelled = std::cell::Cell::new(false);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut session = preview_pane(PreviewerDrop {
                queued: None,
                cancelled: &cancelled,
            });
            session.update_at(Some(0), picker.matches(), Instant::now());
            if unwind {
                panic!("session panic");
            }
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(cancelled.get());
    }
}

#[test]
fn line_number_events_preserve_raw_offsets_and_only_redraw_on_changes() {
    let picker = picker(["alpha"]);
    for (width, height) in [(0, 0), (0, 8), (20, 0), (20, 8)] {
        let cx = Context::new(picker.matches())
            .with_rank(Some(0))
            .with_preview_enabled(true);
        for base in [false, true] {
            let mut session = preview_pane(TestPreviewer {
                line_numbers: base,
                ..TestPreviewer::default()
            });
            session
                .state
                .preview
                .resize(Some(Dimensions { width, height }));
            assert!(session.update(&cx, true, Instant::now()));
            let cached = session
                .state
                .preview
                .cache
                .peek_mut(&id_at(&picker, 0))
                .unwrap();
            let pane = cached.pane_mut().unwrap();
            assert_eq!(pane.line_numbers(), base);
            pane.set_offset(usize::MAX, usize::MAX);
            for (event, expected, changed) in [
                (PreviewEvent::SetLineNumbers(base), base, false),
                (PreviewEvent::ToggleLineNumbers, !base, true),
                (PreviewEvent::ToggleLineNumbers, base, true),
                (PreviewEvent::SetLineNumbers(!base), !base, true),
                (PreviewEvent::SetLineNumbers(!base), !base, false),
            ] {
                assert_eq!(session.handle(event, &cx), changed);
                let pane = session.state.preview.cached().unwrap().pane().unwrap();
                assert_eq!(pane.line_numbers(), expected);
                assert_eq!(pane.offset(), (usize::MAX, usize::MAX));
                assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
                assert!(!session.update(&cx, false, Instant::now()));
            }
            assert_eq!(session.interface.previewer.requested, ["alpha"]);
        }
    }
}

#[test]
fn horizontal_events_follow_layout_and_only_redraw_when_the_offset_changes() {
    let picker = picker(["abcdefghijklmnop", "other"]);
    let mut session = preview_pane(TestPreviewer::default());
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    session.resize(Area {
        width: 12,
        height: 4,
        ..Area::default()
    });
    for (event, expected, changed) in [
        (PreviewEvent::AlignRight, 6, true),
        (PreviewEvent::AlignRight, 6, false),
        (PreviewEvent::AlignLeft, 0, true),
        (PreviewEvent::AlignLeft, 0, false),
        (PreviewEvent::Right(usize::MAX), 6, true),
        (PreviewEvent::Right(1), 6, false),
        (PreviewEvent::ToggleLineNumbers, 6, true),
        (PreviewEvent::Right(usize::MAX), 8, true),
        (PreviewEvent::SetLineNumbers(false), 8, true),
        (PreviewEvent::Right(1), 8, false),
        (PreviewEvent::Left(1), 7, true),
    ] {
        assert_eq!(session.handle_at(event, Some(id_at(&picker, 0))), changed);
        assert_eq!(
            session
                .state
                .preview
                .cached()
                .unwrap()
                .pane()
                .unwrap()
                .offset()
                .0,
            expected
        );
        assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    }
    for width in [30, 0, 12] {
        session.resize(Area {
            width,
            height: 4,
            ..Area::default()
        });
        session.handle_at(PreviewEvent::Right(1), Some(id_at(&picker, 0)));
        assert_eq!(
            session
                .state
                .preview
                .cached()
                .unwrap()
                .pane()
                .unwrap()
                .offset()
                .0,
            7
        );
        assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    }
    session.handle_at(PreviewEvent::Right(1), Some(id_at(&picker, 1)));
    session.handle_at(PreviewEvent::Left(1), None);
    assert!(!session.update_at(Some(0), picker.matches(), Instant::now()));
    assert_eq!(session.state.preview.cache.len(), 1);
    assert_eq!(session.interface.previewer.requested, ["abcdefghijklmnop"]);
    session.restart();
    assert!(session.update_at(Some(0), picker.matches(), Instant::now()));
    assert_eq!(
        session
            .state
            .preview
            .cached()
            .unwrap()
            .pane()
            .unwrap()
            .offset()
            .0,
        0
    );
}
