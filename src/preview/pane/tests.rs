use super::*;
use crate::{PickerOptions, preview::request::QueuedPreviewRequest};

pub(crate) mod support;
use support::*;

#[test]
fn cache_tracks_item_identity_and_preserves_scroll_state() {
    let mut picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    update(&mut session, &mut picker).unwrap();
    let alpha = selected_item(&picker).unwrap();
    session.cache.get_mut(&alpha).unwrap().scroll_position = 7;
    session.cache.get_mut(&alpha).unwrap().horizontal_position = 12;

    picker.update_query("beta");
    settle(&mut picker);
    assert_eq!(
        picker.list_state.layout.selection(picker.engine.snapshot()),
        Some(0)
    );
    update(&mut session, &mut picker).unwrap();
    let beta = selected_item(&picker).unwrap();
    assert_ne!(alpha, beta);

    picker.update_query("");
    settle(&mut picker);
    picker
        .list_state
        .layout
        .set_selection(picker.engine.snapshot(), 0, &picker.list_state.config);
    update(&mut session, &mut picker).unwrap();
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert_eq!(
        session.previewer.focused,
        [Some("alpha"), Some("beta"), Some("alpha")]
    );
    let cached = session.cache.peek(&alpha).unwrap();
    assert_eq!(cached.scroll_position, 7);
    assert_eq!(cached.horizontal_position, 12);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected cached ready preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
}

#[test]
fn refresh_reuses_the_buffer_and_resets_display_settings() {
    let picker = picker(["alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 20,
            ..TestPreviewer::default()
        },
    );
    session.resize_area(Area {
        width: 12,
        height: 5,
        ..Area::default()
    });
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let cached = session.cache.get_mut(&0).unwrap();
    cached.scroll_position = 8;
    cached.horizontal_position = 7;
    cached.line_numbers_override = Some(true);
    let Some(RequestState::Ready(buffer)) = &mut cached.state else {
        panic!("expected ready preview");
    };
    buffer.set_err(true);
    buffer.set_line_numbers(true);
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();

    session.previewer.lines = 4;
    for _ in 0..2 {
        session.handle(PreviewEvent::Refresh, Some(0));
    }
    assert_eq!(session.cache.len(), 1);
    assert_eq!(session.previewer.requested, ["alpha"]);
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    let cached = session.cached().unwrap();
    assert_eq!(cached.scroll_position, 0);
    assert_eq!(cached.horizontal_position, 0);
    assert_eq!(cached.line_numbers_override, None);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected refreshed preview");
    };
    assert_eq!(buffer.lines().len(), 4);
    assert!(!buffer.is_err());
    assert!(!buffer.line_numbers());
    assert_eq!(session.previewer.buffer_allocations[1], allocation);
    assert_eq!(session.previewer.requested, ["alpha", "alpha"]);
    assert_eq!(session.previewer.focused, [Some("alpha")]);
}

#[test]
fn refresh_cancels_outstanding_requests_and_discards_unpolled_results() {
    for stage in 0..4 {
        let picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let mut buffer = session
            .cache
            .peek_mut(&0)
            .unwrap()
            .state
            .take()
            .unwrap()
            .into_buffer();
        let mut allocation = buffer.line(0).unwrap().as_str().as_ptr();
        buffer.clear();
        let (pending, queued) = PreviewRequest {
            buffer,
            epoch: session.epoch,
            snapshot: picker.engine.snapshot(),
            idx: 0,
        }
        .defer();
        session.cache.peek_mut(&0).unwrap().state = Some(RequestState::Pending(pending));
        let mut queued = Some(queued);
        let mut active = None;
        let mut stale = PreviewBuffer::new();
        stale.push_str("stale");
        match stage {
            0 => {}
            1 => active = Some(queued.take().unwrap().start().unwrap()),
            2 => {
                allocation = stale.line(0).unwrap().as_str().as_ptr();
                assert!(queued.take().unwrap().start().unwrap().publish(&mut stale));
            }
            3 => drop(queued.take()),
            _ => unreachable!(),
        }
        assert!(session.area.is_empty());
        session.handle(PreviewEvent::Refresh, Some(0));
        assert_eq!(session.previewer.requested, ["alpha"]);
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
            assert!(queued.start().is_none());
        }
        if let Some(active) = active {
            assert!(active.is_cancelled());
            assert!(!active.publish(&mut stale));
        }
        assert!(
            session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        assert!(
            !session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        let Some(RequestState::Ready(buffer)) = &session.cached().unwrap().state else {
            panic!("expected refreshed preview");
        };
        assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
        assert_eq!(session.previewer.buffer_allocations[1], allocation);
        assert_eq!(session.previewer.requested, ["alpha", "alpha"]);
    }
}

#[test]
fn refresh_only_invalidates_the_selected_cached_preview() {
    let picker = picker(["alpha", "beta", "gamma"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    for idx in [0, 1] {
        session
            .update(Some(idx), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
    }
    for selected in [None, Some(2)] {
        session.handle(PreviewEvent::Refresh, selected);
    }
    assert!(!session.pending_redraw);
    assert_eq!(session.cache.len(), 2);
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    session.cache.peek_mut(&1).unwrap().scroll_position = 7;
    session.handle(PreviewEvent::Refresh, Some(0));
    assert!(
        !session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 7);
    for idx in [0, 1] {
        session
            .update(Some(idx), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta", "alpha"]);
}

#[test]
fn refresh_after_selection_change_does_not_resubmit_a_queued_request() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    for idx in [0, 1] {
        session
            .update(Some(idx), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
    }
    session.previewer.defer = true;
    session.handle(PreviewEvent::Refresh, Some(0));
    assert!(session.previewer.queued.is_empty());
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.previewer.requested, ["alpha", "beta", "alpha"]);
    assert_eq!(session.previewer.queued.len(), 1);
    assert!(!session.previewer.queued[0].is_cancelled());
}

#[test]
fn cache_evicts_the_least_recently_visited_item() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(3),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    let capacity = session.cache.cap().get();
    assert_eq!(capacity, 3);
    let picker = picker(std::iter::repeat_n("item", capacity + 1));
    for selection in 0..capacity as u32 {
        session
            .update(
                Some(selection),
                picker.engine.snapshot(),
                Instant::now(),
                true,
            )
            .unwrap();
    }
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    session
        .update(
            Some(capacity as u32),
            picker.engine.snapshot(),
            Instant::now(),
            true,
        )
        .unwrap();

    assert_eq!(session.cache.len(), capacity);
    assert!(session.cache.contains(&0));
    assert!(!session.cache.contains(&1));
    assert_eq!(session.previewer.requested.len(), capacity + 1);

    session
        .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    assert_eq!(session.previewer.requested.len(), capacity + 2);
    assert_eq!(session.cache.peek(&1).unwrap().scroll_position, 0);
}

#[test]
fn unbounded_cache_retains_previews_and_scroll_state() {
    let options = PickerOptions::new().preview_cache_size(None);
    let mut session = PreviewPane::new(
        &options.preview_config,
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    let item_count = PreviewConfig::default().cache_size.unwrap().get() + 1;
    let picker = picker(std::iter::repeat_n("item", item_count));
    for idx in 0..item_count as u32 {
        session
            .update(Some(idx), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        session.cache.get_mut(&idx).unwrap().scroll_position = idx as usize + 1;
    }
    assert_eq!(session.cache.len(), item_count);

    for idx in 0..item_count as u32 {
        session
            .update(Some(idx), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        assert_eq!(session.cached().unwrap().scroll_position, idx as usize + 1);
    }
    assert_eq!(session.previewer.requested.len(), item_count);
}

#[test]
fn evicted_ready_and_retry_buffers_are_reused_and_scroll_is_reset() {
    let config = PreviewConfig {
        cache_size: std::num::NonZero::new(1),
        ..PreviewConfig::default()
    };
    let mut session = PreviewPane::new(
        &config,
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    let picker = picker(["alpha", "beta"]);
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let cached = session.cache.get_mut(&0).unwrap();
    cached.scroll_position = 7;
    cached.horizontal_position = 12;
    cached.line_numbers_override = Some(true);
    let Some(RequestState::Ready(buffer)) = &mut cached.state else {
        panic!("expected ready preview");
    };
    buffer.push_text("\nprevious contents\n");
    buffer.set_err(true);
    buffer.set_line_numbers(true);
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();

    for (selection, refresh) in [(1, false), (0, true), (1, false)] {
        if refresh {
            session.handle(PreviewEvent::Refresh, session.last_item);
        }
        assert!(
            session
                .update(
                    Some(selection),
                    picker.engine.snapshot(),
                    Instant::now(),
                    true
                )
                .unwrap()
        );
        assert_eq!(
            session.previewer.buffer_allocations.last(),
            Some(&allocation)
        );
        assert_eq!(session.cache.len(), 1);
        assert_eq!(session.cache.peek(&selection).unwrap().scroll_position, 0);
        assert_eq!(
            session.cache.peek(&selection).unwrap().horizontal_position,
            0
        );
        assert_eq!(
            session
                .cache
                .peek(&selection)
                .unwrap()
                .line_numbers_override,
            None
        );
        assert!(
            !session
                .update(
                    Some(selection),
                    picker.engine.snapshot(),
                    Instant::now(),
                    true
                )
                .unwrap()
        );
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
            cache_size: std::num::NonZero::new(1),
            ..PreviewConfig::default()
        };
        let mut session = PreviewPane::new(
            &config,
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let picker = picker(["alpha", "beta", "gamma"]);
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let Some(RequestState::Ready(buffer)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected ready preview");
        };
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();

        session.previewer.defer = true;
        session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        let (queued, active) = if start {
            (None, Some(queued.start().unwrap()))
        } else {
            (Some(queued), None)
        };

        session.previewer.defer = false;
        session
            .update(Some(2), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
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
        cache_size: std::num::NonZero::new(1),
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
    let picker = picker(["alpha", "beta"]);
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
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
    session
        .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
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
            cache_size: std::num::NonZero::new(1),
            ..PreviewConfig::default()
        };
        let mut session = PreviewPane::new(
            &config,
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let picker = picker(["alpha", "beta", "gamma"]);
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let Some(RequestState::Ready(buffer)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected ready preview");
        };
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();

        session.previewer.defer = true;
        session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        if start {
            drop(queued.start().unwrap());
        } else {
            drop(queued);
        }
        session.previewer.defer = false;
        session
            .update(Some(2), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
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
        cache_size: std::num::NonZero::new(1),
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
    let picker = picker(["alpha", "beta"]);
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    assert_eq!(
        session.update(Some(1), picker.engine.snapshot(), Instant::now(), true),
        Err("preview failed")
    );
    assert!(session.previewer.queued[0].is_cancelled());
    assert!(!session.cache.contains(&0));
}

#[test]
fn queued_previews_are_promoted_once_per_revisit() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    for selection in [0, 0, 1, 0, 0, 0] {
        session
            .update(
                Some(selection),
                picker.engine.snapshot(),
                Instant::now(),
                true,
            )
            .unwrap();
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta", "alpha"]);
    assert_eq!(
        session.previewer.focused,
        [Some("alpha"), Some("beta"), Some("alpha")]
    );
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
    let picker = picker(["alpha", "alpha"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    for (selection, changed) in [(0, true), (0, false), (1, true), (0, true), (0, false)] {
        assert_eq!(
            session
                .update(
                    Some(selection),
                    picker.engine.snapshot(),
                    Instant::now(),
                    true
                )
                .unwrap(),
            changed
        );
    }
    assert_eq!(session.previewer.requested, ["alpha"; 2]);
    assert_eq!(session.previewer.focused, [Some("alpha"); 3]);
}

#[test]
fn active_previews_survive_revisits_and_complete_once() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let active = session.previewer.queued.pop().unwrap().start().unwrap();
    session.cache.get_mut(&0).unwrap().scroll_position = 3;
    for selection in [Some(1), None, None, Some(0), Some(0)] {
        session
            .update(selection, picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
    }
    assert!(!active.is_cancelled());
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed alpha");
    buffer.set_err(true);
    assert!(active.publish(&mut buffer));
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    let cached = session.cache.peek(&0).unwrap();
    assert_eq!(cached.scroll_position, 3);
    let Some(RequestState::Ready(buffer)) = &cached.state else {
        panic!("expected completed preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "completed alpha");
    assert!(buffer.is_err());
    assert_eq!(
        session.previewer.focused,
        [Some("alpha"), Some("beta"), None, Some("alpha")]
    );
}

#[test]
fn completed_previews_are_collected_on_revisit_without_resubmission() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        },
    );
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let queued = session.previewer.queued.pop().unwrap();
    session
        .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed alpha");
    assert!(queued.start().unwrap().publish(&mut buffer));
    assert!(
        !session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(matches!(
        session.cache.peek(&0).unwrap().state,
        Some(RequestState::Pending(_))
    ));
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
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
        assert_eq!(session.cached().is_none(), query == "missing");
    }
    assert_eq!(
        session.previewer.requested,
        ["alpha", "beta", "alpha", "alpha"]
    );
    assert!(session.previewer.queued[0].is_cancelled());
    assert!(!session.previewer.queued[1].is_cancelled());
    assert!(session.previewer.queued[2].is_cancelled());
    assert!(!session.previewer.queued[3].is_cancelled());
    assert_eq!(
        session.previewer.focused,
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
fn dropped_queued_and_active_requests_are_retried() {
    for start in [false, true] {
        let picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let queued = session.previewer.queued.pop().unwrap();
        if start {
            drop(queued.start().unwrap());
        } else {
            drop(queued);
        }
        session.cache.get_mut(&0).unwrap().scroll_position = 3;
        assert!(
            !session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        assert!(
            !session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        assert_eq!(session.previewer.requested, ["alpha", "alpha"]);
        assert_eq!(session.previewer.focused, [Some("alpha")]);
        assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 3);
        let Some(RequestState::Pending(pending)) = &session.cache.peek(&0).unwrap().state else {
            panic!("expected replacement request");
        };
        assert_eq!(pending.epoch, session.epoch);
    }
}

#[test]
fn immediately_dropped_requests_are_submitted_at_most_once_per_update() {
    let picker = picker(["alpha"]);
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
        assert_eq!(
            session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap(),
            count == 1
        );
        assert_eq!(session.previewer.requested.len(), count);
    }
    session.previewer.defer = false;
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.previewer.requested.len(), 5);
}

#[test]
fn promotion_and_retry_clear_and_reuse_buffers_without_resetting_scroll() {
    for dropped in [false, true] {
        let picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer::default(),
        );
        let mut buffer = PreviewBuffer::new();
        buffer.push_str("previous contents");
        buffer.newline();
        buffer.set_err(true);
        buffer.set_line_numbers(true);
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();
        let (pending, queued) = PreviewRequest {
            buffer,
            epoch: 0,
            snapshot: picker.engine.snapshot(),
            idx: 0,
        }
        .defer();
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
                horizontal_position: 0,
                line_numbers_override: Some(true),
                state: Some(RequestState::Pending(pending)),
            },
        );
        assert!(
            session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        if let Some(queued) = queued {
            assert!(queued.is_cancelled());
        }
        let cached = session.cache.peek(&0).unwrap();
        assert_eq!(cached.scroll_position, 3);
        assert_eq!(cached.line_numbers_override, Some(true));
        assert!(cached.line_numbers());
        let Some(RequestState::Ready(buffer)) = &cached.state else {
            panic!("expected ready preview");
        };
        assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
        assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);
        assert_eq!(buffer.lines().len(), 1);
        assert!(!buffer.is_err());
    }
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
                session.restart();
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

            session.restart();
            assert!(!update(&mut session, &mut picker).unwrap());
            assert_eq!(session.previewer.focused, [Some("alpha"), None]);
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
    let old_idx = selected_item(&picker).unwrap();

    session.restart();
    assert_eq!(session.last_item, None);
    assert_eq!(session.epoch, 0);
    assert!(session.cache.is_empty());
    assert!(update(&mut session, &mut picker).unwrap());
    assert_eq!(session.previewer.requested, ["alpha", "alpha"]);

    session.restart();
    picker.restart();
    picker.push_batch(["beta"]);
    settle(&mut picker);
    assert_eq!(selected_item(&picker).unwrap(), old_idx);
    session.previewer.defer = true;

    assert!(update(&mut session, &mut picker).unwrap());
    assert!(!update(&mut session, &mut picker).unwrap());
    assert_eq!(session.previewer.requested, ["alpha", "alpha", "beta"]);
    assert!(!session.previewer.queued[0].is_cancelled());
    assert_eq!(
        session.previewer.focused,
        [Some("alpha"), None, Some("alpha"), None, Some("beta")]
    );
}

#[test]
fn scrolling_accumulates_changes_without_advancing_request_priority() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer {
            lines: 30,
            ..TestPreviewer::default()
        },
    );
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    let epoch = session.epoch;
    session.resize_area(Area {
        column: 10,
        row: 0,
        width: 10,
        height: 10,
    });
    for event in [PreviewEvent::Down(1), PreviewEvent::Up(0)] {
        session.handle_event(Some(0), event);
    }
    assert_eq!(session.cache.peek(&0).unwrap().scroll_position, 1);
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.epoch, epoch);
    assert_eq!(session.previewer.requested, ["alpha"]);

    session.handle_event(Some(0), PreviewEvent::Down(1));
    session.previewer.defer = true;
    assert!(
        session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert!(
        !session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.epoch, epoch + 1);
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    assert!(!session.previewer.queued[0].is_cancelled());
}

#[test]
fn restart_and_session_drop_cancel_all_pending_requests() {
    for restart in [true, false] {
        let picker = picker(["alpha", "beta"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                defer: true,
                ..TestPreviewer::default()
            },
        );
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
        let active = session.previewer.queued.pop().unwrap().start().unwrap();
        session
            .update(Some(1), picker.engine.snapshot(), Instant::now(), true)
            .unwrap();
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
        queued: QueuedPreviewRequest<&'static str>,
        cancelled: &'a std::cell::Cell<bool>,
    }

    impl Drop for PreviewerDrop<'_> {
        fn drop(&mut self) {
            self.cancelled.set(self.queued.is_cancelled());
        }
    }

    let cancelled = std::cell::Cell::new(false);
    let picker = picker(["alpha"]);
    let (pending, queued) = PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 0,
        snapshot: picker.engine.snapshot(),
        idx: 0,
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
            horizontal_position: 0,
            line_numbers_override: None,
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
    update(&mut session, &mut picker).unwrap();
    assert!(session.previewer.focused.is_empty());
    assert!(session.previewer.requested.is_empty());
    assert!(session.cache.is_empty());
    assert!(session.cached().is_none());
}

#[test]
fn line_number_events_resolve_preferences_and_only_redraw_on_changes() {
    for base in [false, true] {
        let picker = picker(["alpha"]);
        let mut session = PreviewPane::new(
            &PreviewConfig::default(),
            &crate::PickerChars::new(),
            TestPreviewer {
                line_numbers: base,
                ..TestPreviewer::default()
            },
        );
        assert!(
            session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
        assert_eq!(session.cached().unwrap().line_numbers(), base);
        assert!(session.area.is_empty());
        for (event, expected, changed) in [
            (PreviewEvent::SetLineNumbers(Some(base)), base, false),
            (PreviewEvent::ToggleLineNumbers, !base, true),
            (PreviewEvent::ToggleLineNumbers, base, true),
            (PreviewEvent::SetLineNumbers(None), base, false),
            (PreviewEvent::SetLineNumbers(Some(!base)), !base, true),
            (PreviewEvent::SetLineNumbers(None), base, true),
        ] {
            session.handle(event, Some(0));
            assert_eq!(session.cached().unwrap().line_numbers(), expected);
            assert_eq!(
                session
                    .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                    .unwrap(),
                changed
            );
            assert!(
                !session
                    .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                    .unwrap()
            );
        }
        assert_eq!(session.cached().unwrap().line_numbers_override, None);
        assert_eq!(session.previewer.requested, ["alpha"]);
    }
}

#[test]
fn horizontal_events_follow_layout_and_only_redraw_when_the_offset_changes() {
    let picker = picker(["abcdefghijklmnop"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    session.resize_area(Area {
        width: 12,
        height: 4,
        ..Area::default()
    });
    for (event, expected, changed) in [
        (PreviewEvent::Right(usize::MAX), 6, true),
        (PreviewEvent::Right(1), 6, false),
        (PreviewEvent::ToggleLineNumbers, 6, true),
        (PreviewEvent::Right(usize::MAX), 8, true),
        (PreviewEvent::SetLineNumbers(Some(false)), 8, true),
        (PreviewEvent::Right(1), 8, false),
        (PreviewEvent::Left(1), 7, true),
    ] {
        session.handle(event, Some(0));
        assert_eq!(session.cached().unwrap().horizontal_position, expected);
        assert_eq!(
            session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap(),
            changed
        );
        assert!(
            !session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
    }
    for width in [30, 0, 12] {
        session.resize_area(Area {
            width,
            height: 4,
            ..Area::default()
        });
        session.handle(PreviewEvent::Right(1), Some(0));
        assert_eq!(session.cached().unwrap().horizontal_position, 7);
        assert!(
            !session
                .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
                .unwrap()
        );
    }
    session.handle(PreviewEvent::Right(1), Some(1));
    session.handle(PreviewEvent::Left(1), None);
    assert!(
        !session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.cache.len(), 1);
    assert_eq!(session.previewer.requested, ["abcdefghijklmnop"]);
    session.restart();
    assert!(
        session
            .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
            .unwrap()
    );
    assert_eq!(session.cached().unwrap().horizontal_position, 0);
}

#[test]
fn line_number_overrides_follow_cached_item_identity_and_reset_on_restart() {
    let picker = picker(["alpha", "beta"]);
    let mut session = PreviewPane::new(
        &PreviewConfig::default(),
        &crate::PickerChars::new(),
        TestPreviewer::default(),
    );
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    for selected in [None, Some(1)] {
        session.handle(PreviewEvent::ToggleLineNumbers, selected);
        session.handle(PreviewEvent::SetLineNumbers(Some(true)), selected);
    }
    assert!(!session.pending_redraw);
    assert_eq!(session.cache.len(), 1);
    assert_eq!(session.cached().unwrap().line_numbers_override, None);
    session.handle(PreviewEvent::ToggleLineNumbers, Some(0));
    for selected in [1, 0] {
        session
            .update(
                Some(selected),
                picker.engine.snapshot(),
                Instant::now(),
                true,
            )
            .unwrap();
        assert_eq!(session.cached().unwrap().line_numbers(), selected == 0);
    }
    assert_eq!(session.previewer.requested, ["alpha", "beta"]);
    session.restart();
    session
        .update(Some(0), picker.engine.snapshot(), Instant::now(), true)
        .unwrap();
    assert_eq!(session.cached().unwrap().line_numbers_override, None);
    assert!(!session.cached().unwrap().line_numbers());
}
