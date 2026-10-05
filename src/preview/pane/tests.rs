use super::*;
use crate::{
    Picker, PickerOptions,
    error::PickError,
    event::{Event, EventSource},
    preview::QueuedPreviewRequest,
    render::StrRenderer,
};

#[derive(Default)]
struct TestPreviewer {
    requested: Vec<&'static str>,
    buffer_allocations: Vec<*const u8>,
    queued: Vec<QueuedPreviewRequest>,
    defer: bool,
    drop_requests: bool,
    fail: bool,
    fail_after: Option<usize>,
    lines: usize,
}

impl Preview<&'static str> for TestPreviewer {
    type AbortErr = &'static str;

    fn preview(
        &mut self,
        item: &&'static str,
        request: PreviewRequest,
        timeout: Duration,
    ) -> Result<PreviewResponse, Self::AbortErr> {
        assert!(timeout >= Duration::from_millis(2));
        self.requested.push(item);
        assert_eq!(request.buffer.lines().len(), 1);
        assert_eq!(request.buffer.line(0).unwrap().as_str(), "");
        assert!(!request.buffer.is_err());
        self.buffer_allocations
            .push(request.buffer.line(0).unwrap().as_str().as_ptr());
        if self.fail
            || self
                .fail_after
                .is_some_and(|count| self.requested.len() > count)
        {
            return Err("preview failed");
        }
        if self.defer {
            let (pending, queued) = request.defer();
            if !self.drop_requests {
                self.queued.push(queued);
            }
            Ok(PreviewResponse::Pending(pending))
        } else {
            let mut buffer = request.ready();
            buffer.push_str(item);
            for _ in 1..self.lines {
                buffer.newline();
                buffer.push_str(item);
            }
            Ok(PreviewResponse::Ready(buffer))
        }
    }
}

fn settle(picker: &mut Picker<&'static str, StrRenderer>) {
    let start = std::time::Instant::now();
    loop {
        let status = picker.engine.update(5);
        if status.items_changed {
            picker
                .list_state
                .layout
                .update_items(picker.engine.snapshot(), &picker.list_state.config);
        }
        if !status.matching {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}

fn picker(items: impl IntoIterator<Item = &'static str>) -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .reverse_items(false)
        .sort_results(false)
        .picker(StrRenderer);
    picker.push_batch(items);
    settle(&mut picker);
    picker
        .list_state
        .layout
        .resize(picker.engine.snapshot(), 8, &picker.list_state.config);
    picker
}

fn selected_item<'a>(
    picker: &'a Picker<&'static str, StrRenderer>,
) -> Option<(u32, &'a &'static str)> {
    picker
        .list_state
        .layout
        .selection(picker.engine.snapshot())
        .and_then(|n| picker.engine.get_match(n))
}

fn update(
    application: &mut PreviewPane<TestPreviewer>,
    picker: &mut Picker<&'static str, StrRenderer>,
) -> Result<bool, &'static str> {
    application.update(selected_item(picker), Instant::now())
}

#[test]
fn cache_tracks_item_identity_and_preserves_scroll_state() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    update(&mut session, &mut picker).unwrap();
    let alpha = selected_item(&picker).unwrap().0;
    session.cache.get_mut(&alpha).unwrap().scroll_position = 7;

    picker.update_query("beta");
    settle(&mut picker);
    assert_eq!(
        picker.list_state.layout.selection(picker.engine.snapshot()),
        Some(0)
    );
    update(&mut session, &mut picker).unwrap();
    let beta = selected_item(&picker).unwrap().0;
    assert_ne!(alpha, beta);

    picker.update_query("");
    settle(&mut picker);
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 0, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    let cached = session.cache.peek(&alpha).unwrap();
    assert_eq!(cached.scroll_position, 7);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected cached ready preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
}

#[test]
fn cache_evicts_the_least_recently_visited_item() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(3).unwrap(),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    let capacity = session.cache.cap().get();
    assert_eq!(capacity, 3);
    let mut picker = picker(std::iter::repeat_n("item", capacity + 1));
    for selection in 0..capacity as u32 {
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            selection,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
    }
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 0, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    picker.list_state.layout.set_selection(
        picker.engine.snapshot(),
        capacity as u32,
        &picker.list_state.config,
    );
    update(&mut session, &mut picker).unwrap();

    assert_eq!(session.cache.len(), capacity);
    assert!(session.cache.contains(&0));
    assert!(!session.cache.contains(&1));
    assert_eq!(session.previewer.requested.len(), capacity + 1);

    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    assert_eq!(session.previewer.requested.len(), capacity + 2);
    assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 0);
}

#[test]
fn evicted_ready_buffers_are_reused_and_scroll_is_reset() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(1).unwrap(),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    let mut picker = picker(["alpha", "beta"]);
    update(&mut session, &mut picker).unwrap();
    let cached = session.cache.get_mut(&0).unwrap();
    cached.scroll_position = 7;
    let Some(RequestState::Ready(buffer)) = &mut cached.state else {
        panic!("expected ready preview");
    };
    buffer.push_text("\nprevious contents\n");
    buffer.set_err(true);
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();

    for selection in [1, 0, 1] {
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            selection,
            &picker.list_state.config,
        );
        assert!(update(&mut session, &mut picker).unwrap());
        assert_eq!(
            session.previewer.buffer_allocations.last(),
            Some(&allocation)
        );
        assert_eq!(session.cache.len(), 1);
        assert_eq!(session.cache.peek(&selection).unwrap().scroll_position, 0);
        assert!(!update(&mut session, &mut picker).unwrap());
    }
    assert_eq!(
        session.previewer.requested,
        ["alpha", "beta", "alpha", "beta"]
    );
}

#[test]
fn eviction_cancels_queued_and_active_requests_and_reuses_their_buffers() {
    for start in [false, true] {
        let config = PreviewConfig {
            cache_size: std::num::NonZero::new(1).unwrap(),
            ..PreviewConfig::default()
        };
        let mut session = PreviewPane::new(
            &config,
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let mut picker = picker(["alpha", "beta", "gamma"]);
        update(&mut session, &mut picker).unwrap();
        let Some(RequestState::Ready(buffer)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected ready preview");
        };
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();

        session.previewer.defer = true;
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            1,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        let (queued, active) = if start {
            (None, Some(queued.start().unwrap()))
        } else {
            (Some(queued), None)
        };

        session.previewer.defer = false;
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            2,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        assert_eq!(session.previewer.requested, ["alpha", "beta", "gamma"]);
        assert_eq!(
            session.previewer.buffer_allocations[1..],
            [allocation, allocation]
        );
        assert!(!session.cache.contains(&1));
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
            assert!(queued.start().is_none());
        }
        if let Some(active) = active {
            assert!(active.is_cancelled());
            let mut buffer = PreviewBuffer::new();
            buffer.push_str("unused result");
            assert!(!active.publish(&mut buffer));
            assert_eq!(buffer.line(0).unwrap().as_str(), "unused result");
        }
    }
}

#[test]
fn eviction_reuses_a_published_buffer_before_it_is_polled() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(1).unwrap(),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    let mut picker = picker(["alpha", "beta"]);
    update(&mut session, &mut picker).unwrap();
    let mut buffer = PreviewBuffer::new();
    buffer.push_text("completed alpha\n");
    buffer.set_err(true);
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    assert!(
        session
            .previewer
            .queued
            .pop()
            .unwrap()
            .start()
            .unwrap()
            .publish(&mut buffer)
    );

    session.previewer.defer = false;
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    assert_eq!(
        session.previewer.buffer_allocations.last(),
        Some(&allocation)
    );
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
}

#[test]
fn eviction_reuses_buffers_from_dropped_workers() {
    for start in [false, true] {
        let config = PreviewConfig {
            cache_size: std::num::NonZero::new(1).unwrap(),
            ..PreviewConfig::default()
        };
        let mut session = PreviewPane::new(
            &config,
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let mut picker = picker(["alpha", "beta", "gamma"]);
        update(&mut session, &mut picker).unwrap();
        let Some(RequestState::Ready(buffer)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected ready preview");
        };
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();

        session.previewer.defer = true;
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            1,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        if start {
            drop(queued.start().unwrap());
        } else {
            drop(queued);
        }
        session.previewer.defer = false;
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            2,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        assert_eq!(
            session.previewer.buffer_allocations[1..],
            [allocation, allocation]
        );
        assert_eq!(session.previewer.requested, ["alpha", "beta", "gamma"]);
    }
}

#[test]
fn eviction_cancels_the_old_request_even_if_submission_fails() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(1).unwrap(),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            fail_after: Some(1),
            ..TestPreviewer::default()
        },
    );
    let mut picker = picker(["alpha", "beta"]);
    update(&mut session, &mut picker).unwrap();
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
    assert_eq!(update(&mut session, &mut picker), Err("preview failed"));
    assert!(session.previewer.queued[0].is_cancelled());
    assert!(!session.cache.contains(&0));
}

#[test]
fn queued_previews_are_promoted_once_per_revisit() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    for selection in [0, 0, 1, 0, 0, 0] {
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            selection,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta", "alpha"]);
    assert!(session.previewer.queued[0].is_cancelled());
    assert!(!session.previewer.queued[1].is_cancelled());
    assert!(!session.previewer.queued[2].is_cancelled());
    let Some(RequestState::Pending(pending)) = &session.cache.peek(&0).unwrap().state else {
        panic!("expected pending preview");
    };
    assert_eq!(pending.epoch, session.epoch);
}

#[test]
fn ready_previews_only_report_changes_on_selection() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    for (selection, changed) in [(0, true), (0, false), (1, true), (0, true), (0, false)] {
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            selection,
            &picker.list_state.config,
        );
        assert_eq!(update(&mut session, &mut picker).unwrap(), changed);
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
}

#[test]
fn active_previews_survive_revisits_and_complete_once() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    let active = session.previewer.queued.pop().unwrap().start().unwrap();
    session.cache.get_mut(&0).unwrap().scroll_position = 3;
    for selection in [1, 0, 0] {
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            selection,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
    }
    assert!(!active.is_cancelled());
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed alpha");
    buffer.set_err(true);
    assert!(active.publish(&mut buffer));
    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    let cached = session.cache.peek(&0).unwrap();
    assert_eq!(cached.scroll_position, 3);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected completed preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "completed alpha");
    assert!(buffer.is_err());
}

#[test]
fn completed_previews_are_collected_on_revisit_without_resubmission() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    let queued = session.previewer.queued.pop().unwrap();
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed alpha");
    assert!(queued.start().unwrap().publish(&mut buffer));
    assert!(!update(&mut session, &mut picker).unwrap());
    assert!(matches!(
        session.cache.peek(&0).unwrap().state,
        Some(RequestState::Pending(_))
    ));
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 0, &picker.list_state.config);
    assert!(update(&mut session, &mut picker).unwrap());
    assert!(matches!(
        session.cache.peek(&0).unwrap().state,
        Some(RequestState::Ready(_))
    ));
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
}

#[test]
fn filtering_and_empty_selections_reprioritize_by_item_identity() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
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
        assert_eq!(update(&mut session, &mut picker).unwrap(), changed);
    }
    assert_eq!(
        session.previewer.requested,
        ["alpha", "beta", "alpha", "alpha"]
    );
    assert!(session.previewer.queued[0].is_cancelled());
    assert!(!session.previewer.queued[1].is_cancelled());
    assert!(session.previewer.queued[2].is_cancelled());
    assert!(!session.previewer.queued[3].is_cancelled());
}

#[test]
fn dropped_queued_and_active_requests_are_retried() {
    for start in [false, true] {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        if start {
            drop(queued.start().unwrap());
        } else {
            drop(queued);
        }
        session.cache.get_mut(&0).unwrap().scroll_position = 3;
        assert!(!update(&mut session, &mut picker).unwrap());
        assert!(!update(&mut session, &mut picker).unwrap());
        assert_eq!(session.previewer.requested, ["alpha", "alpha"]);
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 3);
        let Some(RequestState::Pending(pending)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected replacement request");
        };
        assert_eq!(pending.epoch, session.epoch);
    }
}

#[test]
fn immediately_dropped_requests_are_submitted_at_most_once_per_update() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            drop_requests: true,
            ..TestPreviewer::default()
        },
    );
    for count in 1..=4 {
        assert_eq!(update(&mut session, &mut picker).unwrap(), count == 1);
        assert_eq!(session.previewer.requested.len(), count);
    }
    session.previewer.defer = false;
    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    assert_eq!(session.previewer.requested.len(), 5);
}

#[test]
fn promotion_and_retry_clear_and_reuse_buffers_without_resetting_scroll() {
    for dropped in [false, true] {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let mut buffer = PreviewBuffer::new();
        buffer.push_str("previous contents");
        buffer.newline();
        buffer.set_err(true);
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();
        let (pending, queued) = PreviewRequest { buffer, epoch: 0 }.defer();
        let queued = if dropped {
            drop(queued);
            None
        } else {
            Some(queued)
        };
        session.cache.put(
            0,
            Cached {
                scroll_position: 3,
                state: Some(RequestState::Pending(pending)),
            },
        );
        assert!(update(&mut session, &mut picker).unwrap());
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
        }
        let cached = session.cache.peek(&0).unwrap();
        assert_eq!(cached.scroll_position, 3);
        let Some(RequestState::Ready(buffer)) = &cached.state else {
            panic!("expected ready preview");
        };
        assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
        assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);
        assert_eq!(buffer.lines().len(), 1);
        assert!(!buffer.is_err());
    }
}

fn polled_queued_request() -> (crate::preview::PendingPreview, QueuedPreviewRequest) {
    let (pending, queued) = PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 1,
    }
    .defer();
    let Err(BufferNotReady::Queued(RequestState::Pending(pending))) =
        RequestState::Pending(pending).try_into_buffer()
    else {
        panic!("expected queued preview");
    };
    (pending, queued)
}

#[test]
fn promotion_preserves_work_that_starts_after_polling() {
    let (pending, queued) = polled_queued_request();
    let active = queued.start().unwrap();
    let RequestState::Pending(pending) = pending.reprioritize().unwrap() else {
        panic!("expected active preview");
    };
    assert!(!active.is_cancelled());
    let mut buffer = PreviewBuffer::new();
    assert!(active.publish(&mut buffer));
    assert!(RequestState::Pending(pending).try_into_buffer().is_ok());
}

#[test]
fn promotion_collects_work_published_after_polling() {
    let (pending, queued) = polled_queued_request();
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed");
    assert!(queued.start().unwrap().publish(&mut buffer));
    let RequestState::Ready(buffer) = pending.reprioritize().unwrap() else {
        panic!("expected ready preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "completed");
}

#[test]
fn promotion_reclaims_work_dropped_after_polling() {
    let (pending, queued) = polled_queued_request();
    drop(queued);
    assert!(pending.reprioritize().is_err());
}

#[test]
fn restart_preserves_invalidation_until_the_next_update() {
    for defer in [false, true] {
        for restarts in [1, 2] {
            let mut picker = picker(["alpha"]);
            let mut session = PreviewPane::new(
                &PreviewConfig::default(),
                &crate::PickerChars::new(),
                TestPreviewer {
                    defer,
                    ..TestPreviewer::default()
                },
            );
            assert!(update(&mut session, &mut picker).unwrap());

            picker.restart();
            settle(&mut picker);
            assert!(picker.engine.is_empty());
            for _ in 0..restarts {
                session.restart_cache();
            }
            assert!(session.cache.is_empty());
            assert!(
                session
                    .previewer
                    .queued
                    .iter()
                    .all(QueuedPreviewRequest::is_cancelled)
            );
            assert!(update(&mut session, &mut picker).unwrap());
            assert!(!update(&mut session, &mut picker).unwrap());
            assert_eq!(session.epoch, 0);
            assert_eq!(session.previewer.requested, ["alpha"]);

            session.restart_cache();
            assert!(!update(&mut session, &mut picker).unwrap());
        }
    }
}

#[test]
fn restart_with_a_reused_item_id_and_pending_preview_reports_one_change() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    update(&mut session, &mut picker).unwrap();
    let old_idx = selected_item(&picker).unwrap().0;

    session.restart_cache();
    picker.restart();
    picker.push_batch(["beta"]);
    settle(&mut picker);
    assert_eq!(selected_item(&picker).unwrap().0, old_idx);
    session.previewer.defer = true;

    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert!(!session.previewer.queued[0].is_cancelled());
}

#[test]
fn scrolling_accumulates_changes_without_advancing_request_priority() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    update(&mut session, &mut picker).unwrap();
    let epoch = session.epoch;
    session.resize_area(Area {
        column: 10,
        row: 0,
        width: 10,
        height: 10,
    });
    for event in [PreviewEvent::Down(1), PreviewEvent::Up(0)] {
        session.scroll(Some(0), event);
    }
    assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 1);
    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    assert_eq!(session.epoch, epoch);
    assert_eq!(session.previewer.requested, ["alpha"]);

    session.scroll(Some(0), PreviewEvent::Down(1));
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 1, &picker.list_state.config);
    session.previewer.defer = true;
    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    assert_eq!(session.epoch, epoch + 1);
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert!(!session.previewer.queued[0].is_cancelled());
}

#[test]
fn restart_resets_selection_and_priority_bookkeeping() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    update(&mut session, &mut picker).unwrap();
    session.restart_cache();
    assert_eq!(session.last_item, None);
    assert_eq!(session.epoch, 0);
    assert!(session.cache.is_empty());
    assert!(update(&mut session, &mut picker).unwrap());
    assert_eq!(session.previewer.requested, ["alpha", "alpha"]);
}

#[test]
fn restart_and_session_drop_cancel_all_pending_requests() {
    for restart in [true, false] {
        let mut picker = picker(["alpha", "beta"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        let active = session.previewer.queued.pop().unwrap().start().unwrap();
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            1,
            &picker.list_state.config,
        );
        update(&mut session, &mut picker).unwrap();
        let queued = session.previewer.queued.pop().unwrap();

        if restart {
            session.restart_cache();
            assert!(session.cache.is_empty());
            assert_eq!(session.last_item, None);
            assert_eq!(session.epoch, 0);
            session.restart_cache();
        } else {
            drop(session);
        }

        assert!(queued.is_cancelled());
        assert!(queued.start().is_none());
        assert!(active.is_cancelled());
        let mut buffer = PreviewBuffer::new();
        buffer.push_str("unused result");
        assert!(!active.publish(&mut buffer));
        assert_eq!(buffer.line(0).unwrap().as_str(), "unused result");
    }
}

#[test]
fn session_cancels_requests_before_dropping_its_previewer() {
    struct PreviewerDrop<'a> {
        queued: QueuedPreviewRequest,
        cancelled: &'a std::cell::Cell<bool>,
    }

    impl Drop for PreviewerDrop<'_> {
        fn drop(&mut self) {
            self.cancelled.set(self.queued.is_cancelled());
        }
    }

    let cancelled = std::cell::Cell::new(false);
    let (pending, queued) = PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 0,
    }
    .defer();
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        PreviewerDrop {
            queued,
            cancelled: &cancelled,
        },
    );
    session.cache.put(
        0,
        Cached {
            scroll_position: 0,
            state: Some(RequestState::Pending(pending)),
        },
    );

    drop(session);
    assert!(cancelled.get());
}

#[test]
fn an_empty_match_list_does_not_request_previews() {
    let mut picker = picker([]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    update(&mut session, &mut picker).unwrap();
    assert!(session.previewer.requested.is_empty());
    assert!(session.cache.is_empty());
}

#[test]
fn current_entry_exposes_pending_and_ready_states_without_polling() {
    let mut picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    assert!(session.cached().is_none());
    update(&mut session, &mut picker).unwrap();
    assert!(matches!(
        session.cached().unwrap().state,
        Some(RequestState::Pending(_))
    ));

    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed alpha");
    assert!(
        session
            .previewer
            .queued
            .pop()
            .unwrap()
            .start()
            .unwrap()
            .publish(&mut buffer)
    );
    assert!(matches!(
        session.cached().unwrap().state,
        Some(RequestState::Pending(_))
    ));

    update(&mut session, &mut picker).unwrap();
    let cached = session.cached().unwrap();
    assert_eq!(cached.scroll_position, 0);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected completed preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "completed alpha");

    picker.update_query("missing");
    settle(&mut picker);
    update(&mut session, &mut picker).unwrap();
    assert!(session.cached().is_none());
}

#[cfg(feature = "unstable-backend")]
mod picker_loop {
    use std::collections::VecDeque;

    use super::*;
    use crate::{
        Terminal,
        event::{MatchListEvent, RecvError},
    };

    struct Events(VecDeque<Result<Event<&'static str>, RecvError>>);

    impl EventSource for Events {
        type AbortErr = &'static str;

        fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
            Err(RecvError::Timeout)
        }

        fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
            self.0.pop_front().expect("event script exhausted")
        }
    }

    struct CallbackEvents<F>(F);

    impl<F: FnMut() -> Result<Event<&'static str>, RecvError>> EventSource for CallbackEvents<F> {
        type AbortErr = &'static str;

        fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
            Err(RecvError::Timeout)
        }

        fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
            (self.0)()
        }
    }

    #[derive(Default)]
    struct TestTerminal {
        output: Vec<u8>,
        cleanups: usize,
        sizes: VecDeque<(u16, u16)>,
        changed: Vec<bool>,
        fail_write: bool,
        fail_cleanup: bool,
    }

    impl io::Write for TestTerminal {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                return Err(io::Error::other("write failed"));
            }
            self.output.extend_from_slice(bytes);
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
            self.cleanups += 1;
            if self.fail_cleanup {
                return Err(io::Error::other("cleanup failed"));
            }
            Ok(())
        }

        fn size(&mut self) -> io::Result<(u16, u16)> {
            if self.sizes.len() > 1 {
                Ok(self.sizes.pop_front().unwrap())
            } else {
                Ok(self.sizes.front().copied().unwrap_or((20, 10)))
            }
        }

        fn end_frame(&mut self, changed: bool) -> io::Result<()> {
            self.changed.push(changed);
            Ok(())
        }
    }

    #[test]
    fn interleaved_navigation_and_scrolling_target_the_buffered_item() {
        for reversed in [false, true] {
            let mut picker = PickerOptions::new()
                .reverse_items(false)
                .reversed(reversed)
                .picker(StrRenderer);
            picker.push_batch(["alpha", "beta"]);
            settle(&mut picker);
            picker
                .list_state
                .layout
                .resize(picker.engine.snapshot(), 8, &picker.list_state.config);
            let mut session = PreviewPane::new(
                &PreviewConfig::default(),
                &crate::PickerChars::new(),
                TestPreviewer {
                    lines: 30,
                    ..TestPreviewer::default()
                },
            );
            picker.list_state.layout.set_selection(
                picker.engine.snapshot(),
                1,
                &picker.list_state.config,
            );
            update(&mut session, &mut picker).unwrap();
            picker.list_state.layout.set_selection(
                picker.engine.snapshot(),
                0,
                &picker.list_state.config,
            );
            let (next, previous) = if reversed {
                (MatchListEvent::Down(1), MatchListEvent::Up(1))
            } else {
                (MatchListEvent::Up(1), MatchListEvent::Down(1))
            };
            let events = Events(VecDeque::from([
                Ok(Event::Preview(PreviewEvent::Up(2))),
                Ok(Event::Preview(PreviewEvent::Down(2))),
                Ok(Event::MatchList(next)),
                Ok(Event::Preview(PreviewEvent::PageDown(1))),
                Ok(Event::Preview(PreviewEvent::Up(1))),
                Ok(Event::MatchList(previous)),
                Ok(Event::Preview(PreviewEvent::Down(1))),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]));
            picker
                .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
                .unwrap();
            assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 3);
            assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 7);
            assert_eq!(session.previewer.requested, ["beta", "alpha"]);
        }
    }

    #[test]
    fn scrolling_a_filtered_match_uses_its_item_id() {
        let mut picker = picker(["alpha", "beta"]);
        picker.update_query("beta");
        settle(&mut picker);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 8);
        assert!(!session.cache.contains(&0));
    }

    #[test]
    fn scrolling_a_missing_preview_does_not_request_it() {
        let mut picker = picker(["alpha", "beta"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        let events = Events(VecDeque::from([
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
            .unwrap();
        assert_eq!(session.previewer.requested, ["alpha"]);
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 0);
        assert!(!session.cache.contains(&1));
    }

    #[test]
    fn scrolling_pending_previews_is_ignored_by_the_loop() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        let mut terminal = TestTerminal::default();
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 0);
        assert_eq!(session.previewer.requested, ["alpha"]);
        assert_eq!(terminal.changed, [false]);
    }

    #[test]
    fn idle_frames_collect_completed_previews_and_enable_scrolling() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        let mut queued = session.previewer.queued.pop();
        let mut step = 0;
        let events = CallbackEvents(move || {
            step += 1;
            match step {
                1 | 4 | 5 => Err(RecvError::Timeout),
                2 => {
                    let mut buffer = PreviewBuffer::new();
                    for _ in 1..30 {
                        buffer.newline();
                    }
                    assert!(queued.take().unwrap().start().unwrap().publish(&mut buffer));
                    Err(RecvError::Timeout)
                }
                3 => Ok(Event::Preview(PreviewEvent::Down(1))),
                _ => Ok(Event::Quit),
            }
        });
        let mut terminal = TestTerminal::default();
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(terminal.changed, [false, true, true, false]);
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 1);
        assert_eq!(session.previewer.requested, ["alpha"]);

        let output = String::from_utf8(terminal.output).unwrap();
        assert_eq!(output.matches("alpha").count(), 1);
        assert_eq!(output.matches('>').count(), 1);
        assert_eq!(output.matches('╭').count(), 3);
    }

    #[test]
    fn batched_navigation_back_to_the_same_item_does_not_promote() {
        let mut picker = picker(["alpha", "beta"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        let events = Events(VecDeque::from([
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        let mut terminal = TestTerminal::default();
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(terminal.changed, [false]);
        assert_eq!(session.previewer.requested, ["alpha"]);
        assert!(!session.previewer.queued[0].is_cancelled());
    }

    #[test]
    fn retry_and_promotion_errors_abort_and_clean_up_the_terminal() {
        for retry in [false, true] {
            let mut picker = picker(["alpha", "beta"]);
            let mut previewer = TestPreviewer {
                defer: true,
                drop_requests: retry,
                fail_after: Some(if retry { 1 } else { 2 }),
                ..TestPreviewer::default()
            };
            let mut terminal = TestTerminal::default();
            let events = if retry {
                VecDeque::from([Err(RecvError::Timeout)])
            } else {
                VecDeque::from([
                    Ok(Event::MatchList(MatchListEvent::Up(1))),
                    Err(RecvError::Timeout),
                    Ok(Event::MatchList(MatchListEvent::Down(1))),
                    Err(RecvError::Timeout),
                ])
            };
            let mut preview_picker = picker.with_preview(&mut previewer);
            let result = preview_picker.pick_with_terminal_io(Events(events), &mut terminal);
            assert!(matches!(result, Err(PickError::Aborted("preview failed"))));
            assert_eq!(terminal.cleanups, 1);
            if retry {
                assert_eq!(previewer.requested, ["alpha", "alpha"]);
            } else {
                assert_eq!(previewer.requested, ["alpha", "beta", "alpha"]);
                assert!(previewer.queued[0].is_cancelled());
                assert!(previewer.queued[1].is_cancelled());
            }
        }
    }

    #[test]
    fn scroll_changes_are_reported_without_redrawing_the_match_list() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        let mut terminal = TestTerminal::default();
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 8);
        assert_eq!(terminal.changed, [true]);

        let output = String::from_utf8(terminal.output).unwrap();
        assert_eq!(output.matches("alpha").count(), 17);
        assert_eq!(output.matches('▌').count(), 1);
        assert_eq!(output.matches('>').count(), 1);
        assert_eq!(output.matches('╭').count(), 2);
    }

    #[test]
    fn revealing_the_pane_on_a_width_change_reclamps_the_offset() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        session.cache.get_mut(&0).unwrap().scroll_position = 25;
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([(5, 10), (20, 10)]),
            ..TestTerminal::default()
        };
        let events = Events(VecDeque::from([
            Ok(Event::Redraw),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 22);
    }

    #[test]
    fn resizing_reclamps_the_offset_and_changes_page_height() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([(20, 10), (20, 14)]),
            ..TestTerminal::default()
        };
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
            Ok(Event::Redraw),
            Err(RecvError::Timeout),
            Ok(Event::Preview(PreviewEvent::PageUp(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 6);
    }

    #[test]
    fn initial_layout_resizes_retained_preview_state() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        session
            .cache
            .get_mut(&0)
            .unwrap()
            .scroll(PreviewEvent::Down(usize::MAX), 4);
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 26);

        picker
            .pick_impl::<_, _, (), _>(
                Events(VecDeque::from([Ok(Event::Quit)])),
                &mut TestTerminal::default(),
                &mut session,
            )
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 22);
    }

    #[test]
    fn resizing_clamps_cached_previews_before_they_are_revisited() {
        let mut picker = picker(["alpha", "beta"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        update(&mut session, &mut picker).unwrap();
        session
            .cache
            .get_mut(&0)
            .unwrap()
            .scroll(PreviewEvent::Down(usize::MAX), 8);
        picker.list_state.layout.set_selection(
            picker.engine.snapshot(),
            1,
            &picker.list_state.config,
        );
        let events = Events(VecDeque::from([
            Ok(Event::Redraw),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        let mut terminal = TestTerminal {
            sizes: VecDeque::from([(20, 10), (20, 14)]),
            ..TestTerminal::default()
        };
        picker
            .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 18);
        assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    }

    #[test]
    fn scrolling_down_down_up_at_the_bottom_moves_up_one_line() {
        let mut picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                lines: 30,
                ..TestPreviewer::default()
            },
        );
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Down(usize::MAX))),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::Quit),
        ]));
        picker
            .pick_impl::<_, _, (), _>(events, &mut TestTerminal::default(), &mut session)
            .unwrap();
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 21);
    }

    #[test]
    fn zero_sized_panes_ignore_scrolling() {
        for size in [(0, 10), (5, 10), (20, 2), (20, 0)] {
            let mut picker = picker(["alpha"]);
            let mut session = PreviewPane::new(
                &PreviewConfig::default(),
                &crate::PickerChars::new(),
                TestPreviewer {
                    lines: 30,
                    ..TestPreviewer::default()
                },
            );
            let mut terminal = TestTerminal {
                sizes: VecDeque::from([size]),
                ..TestTerminal::default()
            };
            let events = Events(VecDeque::from([
                Ok(Event::Preview(PreviewEvent::Down(1))),
                Ok(Event::Preview(PreviewEvent::PageDown(1))),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]));
            picker
                .pick_impl::<_, _, (), _>(events, &mut terminal, &mut session)
                .unwrap();
            assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 0);
            assert_eq!(terminal.changed, [false]);
        }
    }

    #[test]
    fn ordinary_pickers_ignore_preview_events() {
        let mut picker = picker(["alpha"]);
        let mut terminal = TestTerminal::default();
        let events = Events(VecDeque::from([
            Ok(Event::Preview(PreviewEvent::Down(1))),
            Ok(Event::Preview(PreviewEvent::PageDown(1))),
            Err(RecvError::Timeout),
            Ok(Event::Select),
        ]));
        let selected = picker.pick_with_terminal_io(events, &mut terminal).unwrap();
        assert_eq!(selected, Some(&"alpha"));
        assert_eq!(terminal.changed, [false]);
    }

    fn navigation() -> Events {
        Events(VecDeque::from([
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Up(1))),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::Down(1))),
            Err(RecvError::Timeout),
            Ok(Event::Select),
        ]))
    }

    #[test]
    fn preview_picker_uses_the_shared_loop_and_draws_a_pane() {
        let mut picker = picker(["alpha", "beta"]);
        let mut plain_terminal = TestTerminal::default();
        let selected = picker
            .pick_with_terminal_io(navigation(), &mut plain_terminal)
            .unwrap();
        assert_eq!(selected, Some(&"alpha"));

        let mut previewer = TestPreviewer::default();
        let mut terminal = TestTerminal::default();
        let mut preview_picker = picker.with_preview(&mut previewer);
        let selected = preview_picker
            .pick_with_terminal_io(navigation(), &mut terminal)
            .unwrap();
        assert_eq!(selected, Some(&"alpha"));
        assert_eq!(previewer.requested, ["alpha", "beta"]);
        assert!(
            !String::from_utf8(plain_terminal.output)
                .unwrap()
                .contains('╭')
        );
        assert!(String::from_utf8(terminal.output).unwrap().contains('╭'));
        assert_eq!(terminal.cleanups, 1);
    }

    #[test]
    fn preview_picker_preserves_multiple_selection() {
        let mut picker = picker(["alpha", "beta"]);
        let mut preview_picker = picker.with_preview(TestPreviewer::default());
        let events = Events(VecDeque::from([
            Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
            Err(RecvError::Timeout),
            Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
            Ok(Event::Select),
        ]));
        let selected = preview_picker
            .pick_multi_with_terminal_io(events, &mut TestTerminal::default())
            .unwrap();
        assert_eq!(
            selected.iter().copied().collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
    }

    #[test]
    fn preview_errors_abort_and_clean_up_the_terminal() {
        let mut picker = picker(["alpha"]);
        let mut preview_picker = picker.with_preview(TestPreviewer {
            fail: true,
            ..TestPreviewer::default()
        });
        let mut terminal = TestTerminal::default();
        let result = preview_picker.pick_with_terminal_io(Events(VecDeque::new()), &mut terminal);
        assert!(matches!(result, Err(PickError::Aborted("preview failed"))));
        assert_eq!(terminal.cleanups, 1);
    }

    #[test]
    fn picker_exits_cancel_all_pending_requests() {
        for (exit, succeeds) in [
            (Ok(Event::Select), true),
            (Ok(Event::Quit), true),
            (Ok(Event::QuitPromptEmpty), true),
            (Ok(Event::UserInterrupt), false),
            (Ok(Event::Abort("aborted")), false),
            (Ok(Event::Restart), false),
            (Err(RecvError::Disconnected), false),
            (Err(RecvError::IO(io::Error::other("read failed"))), false),
        ] {
            let mut picker = picker(["alpha", "beta"]);
            let mut previewer = TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            };
            let events = Events(VecDeque::from([
                Ok(Event::MatchList(MatchListEvent::Up(1))),
                Err(RecvError::Timeout),
                exit,
            ]));
            let mut terminal = TestTerminal::default();
            let mut preview_picker = picker.with_preview(&mut previewer);
            let result = preview_picker.pick_with_terminal_io(events, &mut terminal);

            assert_eq!(result.is_ok(), succeeds);
            assert_eq!(terminal.cleanups, 1);
            assert_eq!(previewer.queued.len(), 2);
            assert!(
                previewer
                    .queued
                    .iter()
                    .all(QueuedPreviewRequest::is_cancelled)
            );
        }
    }

    #[test]
    fn terminal_errors_cancel_pending_requests() {
        for fail_write in [true, false] {
            let mut picker = picker(["alpha"]);
            let mut previewer = TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            };
            let mut terminal = TestTerminal {
                fail_write,
                fail_cleanup: !fail_write,
                ..TestTerminal::default()
            };
            let mut preview_picker = picker.with_preview(&mut previewer);
            let result = preview_picker
                .pick_with_terminal_io(Events(VecDeque::from([Ok(Event::Quit)])), &mut terminal);

            assert!(matches!(result, Err(PickError::IO(_))));
            assert_eq!(terminal.cleanups, 1);
            assert!(previewer.queued[0].is_cancelled());
        }
    }

    #[test]
    fn unwinding_cancels_pending_requests() {
        let mut picker = picker(["alpha"]);
        let mut previewer = TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        };
        let mut terminal = TestTerminal::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut preview_picker = picker.with_preview(&mut previewer);
            let _ = preview_picker.pick_with_terminal_io(
                CallbackEvents(|| panic!("event source panic")),
                &mut terminal,
            );
        }));

        assert!(panic.is_err());
        assert_eq!(terminal.cleanups, 1);
        assert!(previewer.queued[0].is_cancelled());
    }

    #[test]
    fn each_pick_session_starts_with_a_fresh_cache() {
        let mut picker = picker(["alpha"]);
        let mut previewer = TestPreviewer::default();
        let mut preview_picker = picker.with_preview(&mut previewer);
        for _ in 0..2 {
            preview_picker
                .pick_with_terminal_io(
                    Events(VecDeque::from([Ok(Event::Quit)])),
                    &mut TestTerminal::default(),
                )
                .unwrap();
        }
        assert_eq!(previewer.requested, ["alpha", "alpha"]);
    }

    #[test]
    fn restart_invalidates_cached_item_ids() {
        struct RestartEvents {
            injectors: crate::Observer<crate::Injector<&'static str, StrRenderer>>,
            statuses: crate::Observer<crate::event::PickerStatus>,
            step: usize,
            started: Instant,
        }

        impl EventSource for RestartEvents {
            type AbortErr = &'static str;

            fn try_recv(&mut self) -> Result<Event<Self::AbortErr>, RecvError> {
                Err(RecvError::Timeout)
            }

            fn recv_timeout(&mut self, _: Duration) -> Result<Event<Self::AbortErr>, RecvError> {
                assert!(self.started.elapsed() < Duration::from_secs(5));
                match self.step {
                    0 => {
                        self.step = 1;
                        Ok(Event::Restart)
                    }
                    1 => {
                        self.injectors.try_recv().unwrap().push("beta");
                        self.step = 2;
                        Ok(Event::Status { id: 0 })
                    }
                    2 => {
                        self.step = 3;
                        Err(RecvError::Timeout)
                    }
                    _ => {
                        if self.statuses.try_recv().unwrap().matched_item_count == 1 {
                            Ok(Event::Select)
                        } else {
                            self.step = 2;
                            Ok(Event::Status { id: 0 })
                        }
                    }
                }
            }
        }

        let mut picker = picker(["alpha"]);
        let events = RestartEvents {
            injectors: picker.injector_observer(false),
            statuses: picker.status_observer(),
            step: 0,
            started: Instant::now(),
        };
        let mut previewer = TestPreviewer::default();
        let mut preview_picker = picker.with_preview(&mut previewer);
        let selected = preview_picker
            .pick_with_terminal_io(events, &mut TestTerminal::default())
            .unwrap();
        assert_eq!(selected, Some(&"beta"));
        assert_eq!(previewer.requested, ["alpha", "beta"]);
    }
}
