use std::{
    cell::{Cell, RefCell},
    num::NonZero,
    time::{Duration, Instant},
};

use crate::{
    interface::{CachedPreview, Interface},
    preview::{Content, DEFAULT_CACHE_SIZE, Pane, PreviewEvent, SyncPreviewer},
    session::{Components, Dimensions, Exit},
};

mod prefetch;
pub(crate) mod support;
use support::*;

#[test]
fn synchronous_refresh_clears_the_pane_and_preserves_generated_presentation() {
    let driver = driver(["alpha"]);
    let cx = context(&driver, Some(0));
    let generated = Cell::new(0);
    let mut interface = CachedPreview::new_single(SyncPreviewer(|item: &&str, pane: &mut Pane| {
        assert_eq!(pane.buffer().lines().len(), 1);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
        assert_eq!(pane.offset(), (0, 0));
        assert!(!pane.line_numbers());
        assert!(!pane.is_err());
        generated.set(generated.get() + 1);
        pane.push_text(item);
        pane.set_offset(5, 17);
        pane.set_line_numbers(true);
        pane.set_err(true);
    }));
    let mut state = interface.init(&cx);
    for count in 1..=2 {
        assert_eq!(before(&mut interface, &mut state, &cx), Components::PREVIEW);
        assert!(before(&mut interface, &mut state, &cx).is_none());
        let pane = ready(interface.preview_content(&state));
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
        assert_eq!(pane.offset(), (5, 17));
        assert!(pane.line_numbers());
        assert!(pane.is_err());
        assert_eq!(generated.get(), count);
        let _ = interface.preview_event(&mut state, PreviewEvent::SetLineNumbers(false), &cx);
        let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &cx);
        let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &cx);
        assert_eq!(generated.get(), count);
    }
}

#[test]
fn cached_presentation_follows_item_identity_through_filtering_duplicates_and_restart() {
    let mut driver = driver(["alpha", "beta", "alpha"]);
    let first = driver.matches().get(0).unwrap().id();
    let previewer = RefCell::new(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let mut state = interface.init(&context(&driver, Some(0)));
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 12,
            height: 5,
        }),
    );
    let _ = before(&mut interface, &mut state, &context(&driver, Some(0)));
    for event in [PreviewEvent::Down(7), PreviewEvent::ToggleLineNumbers] {
        let _ = interface.preview_event(&mut state, event, &context(&driver, Some(0)));
    }

    driver.update_query("beta");
    driver.filter().unwrap();
    let beta = context(&driver, Some(0));
    let _ = before(&mut interface, &mut state, &beta);
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 0));
    assert_eq!(previewer.borrow().ids[1], beta.hovered().unwrap().id());

    driver.update_query("");
    driver.filter().unwrap();
    for (rank, offset, numbers) in [(2, (0, 0), false), (0, (0, 7), true)] {
        let _ = before(&mut interface, &mut state, &context(&driver, Some(rank)));
        let pane = ready(interface.preview_content(&state));
        assert_eq!(pane.offset(), offset);
        assert_eq!(pane.line_numbers(), numbers);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
    }
    assert_eq!(previewer.borrow().requested, ["alpha", "beta", "alpha"]);
    assert_ne!(previewer.borrow().ids[0], previewer.borrow().ids[2]);

    interface.on_restart(&mut state, &context(&driver, Some(0)));
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    driver.restart();
    driver.push_batch(["alpha"]);
    driver.filter().unwrap();
    let _ = before(&mut interface, &mut state, &context(&driver, Some(0)));
    let pane = ready(interface.preview_content(&state));
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert_ne!(*previewer.borrow().ids.last().unwrap(), first);
    assert_eq!(
        previewer.borrow().requested,
        ["alpha", "beta", "alpha", "alpha"]
    );
}

#[test]
fn unbounded_cache_retains_more_than_the_default_capacity() {
    let count = DEFAULT_CACHE_SIZE.unwrap().get() + 1;
    let driver = driver(std::iter::repeat_n("item", count));
    let previewer = RefCell::new(TestPreviewer::default());
    let mut interface = CachedPreview::new_single(&previewer).cache_size(None);
    let mut state = interface.init(&context(&driver, Some(0)));
    for _ in 0..2 {
        for rank in 0..count as u32 {
            let _ = before(&mut interface, &mut state, &context(&driver, Some(rank)));
            assert_eq!(text(interface.preview_content(&state)), "item");
        }
    }
    assert_eq!(previewer.borrow().requested.len(), count);
}

#[test]
fn refresh_targets_the_event_item_and_waits_until_it_is_displayed() {
    let driver = driver(["alpha", "beta", "missing"]);
    let previewer = RefCell::new(TestPreviewer {
        lines: 20,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let alpha = context(&driver, Some(0));
    let beta = context(&driver, Some(1));
    let mut state = interface.init(&alpha);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 12,
            height: 5,
        }),
    );
    let _ = before(&mut interface, &mut state, &alpha);
    let _ = before(&mut interface, &mut state, &beta);
    let _ = interface.preview_event(&mut state, PreviewEvent::Down(7), &beta);
    for rank in [None, Some(2)] {
        assert!(
            interface
                .preview_event(&mut state, PreviewEvent::Refresh, &context(&driver, rank))
                .is_none()
        );
    }
    let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &alpha);
    assert!(before(&mut interface, &mut state, &beta).is_none());
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 7));
    assert_eq!(previewer.borrow().requested, ["alpha", "beta"]);

    previewer.borrow_mut().defer = true;
    let _ = before(&mut interface, &mut state, &alpha);
    let _ = before(&mut interface, &mut state, &alpha);
    assert!(matches!(
        interface.preview_content(&state),
        Content::Loading
    ));
    let _ = before(&mut interface, &mut state, &beta);
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 7));
    assert_eq!(previewer.borrow().requested, ["alpha", "beta", "alpha"]);
    assert!(!previewer.borrow().queued[0].is_cancelled());
}

#[test]
fn revisits_promote_queued_requests_but_preserve_active_work() {
    let driver = driver(["alpha", "beta"]);
    let previewer = RefCell::new(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let alpha = context(&driver, Some(0));
    let beta = context(&driver, Some(1));
    let mut state = interface.init(&alpha);
    for cx in [&alpha, &alpha, &beta, &alpha, &alpha] {
        let _ = before(&mut interface, &mut state, cx);
    }
    assert_eq!(previewer.borrow().requested, ["alpha", "beta", "alpha"]);
    assert!(previewer.borrow().queued[0].is_cancelled());
    let active = previewer
        .borrow_mut()
        .take_request("alpha")
        .start()
        .unwrap();
    let hidden = context(&driver, Some(1)).with_active_components(Components {
        preview: false,
        ..Components::ALL
    });
    for cx in [&hidden, &beta, &alpha, &alpha] {
        let _ = before(&mut interface, &mut state, cx);
    }
    assert!(!active.is_cancelled());
    publish(active);
    assert_eq!(
        before(&mut interface, &mut state, &alpha),
        Components::PREVIEW
    );
    assert!(before(&mut interface, &mut state, &alpha).is_none());
    assert_eq!(text(interface.preview_content(&state)), "alpha");
    assert_eq!(
        previewer.borrow().requested,
        ["alpha", "beta", "alpha", "beta"]
    );
}

#[test]
fn dropped_requests_retry_once_per_frame_and_can_eventually_complete() {
    let driver = driver(["alpha"]);
    let previewer = RefCell::new(TestPreviewer {
        defer: true,
        drop_requests: true,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    for count in 1..=3 {
        let _ = before(&mut interface, &mut state, &cx);
        assert_eq!(previewer.borrow().requested.len(), count);
        assert!(matches!(
            interface.preview_content(&state),
            Content::Loading
        ));
    }
    previewer.borrow_mut().drop_requests = false;
    let _ = before(&mut interface, &mut state, &cx);
    drop(
        previewer
            .borrow_mut()
            .take_request("alpha")
            .start()
            .unwrap(),
    );
    let _ = before(&mut interface, &mut state, &cx);
    let _ = before(&mut interface, &mut state, &cx);
    assert_eq!(previewer.borrow().requested.len(), 5);
    publish(
        previewer
            .borrow_mut()
            .take_request("alpha")
            .start()
            .unwrap(),
    );
    assert_eq!(before(&mut interface, &mut state, &cx), Components::PREVIEW);
    assert!(before(&mut interface, &mut state, &cx).is_none());
    assert_eq!(text(interface.preview_content(&state)), "alpha");
}

#[test]
fn refresh_and_eviction_cancel_outstanding_work() {
    for evict in [false, true] {
        let driver = driver(["alpha", "beta"]);
        let previewer = RefCell::new(TestPreviewer {
            defer: true,
            ..TestPreviewer::default()
        });
        let mut interface = CachedPreview::new_single(&previewer).cache_size(NonZero::new(1));
        let alpha = context(&driver, Some(0));
        let mut state = interface.init(&alpha);
        let _ = before(&mut interface, &mut state, &alpha);
        let queued = previewer.borrow_mut().take_request("alpha");
        let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &alpha);
        assert!(queued.is_cancelled());
        assert_eq!(previewer.borrow().requested, ["alpha"]);
        let _ = before(&mut interface, &mut state, &alpha);
        let active = previewer
            .borrow_mut()
            .take_request("alpha")
            .start()
            .unwrap();

        previewer.borrow_mut().defer = false;
        let cx = context(&driver, Some(u32::from(evict)));
        if !evict {
            let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &cx);
        }
        let _ = before(&mut interface, &mut state, &cx);
        assert!(active.is_cancelled());
        let expected = if evict { "beta" } else { "alpha" };
        assert_eq!(text(interface.preview_content(&state)), expected);
        assert_eq!(previewer.borrow().requested, ["alpha", "alpha", expected]);
    }
}

#[test]
fn pending_events_are_ignored_until_a_frame_collects_the_publication() {
    let driver = driver(["alpha"]);
    let previewer = RefCell::new(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 3,
        }),
    );
    let _ = before(&mut interface, &mut state, &cx);
    assert!(
        interface
            .preview_event(&mut state, PreviewEvent::Down(1), &cx)
            .is_none()
    );
    let active = previewer
        .borrow_mut()
        .take_request("alpha")
        .start()
        .unwrap();
    let mut pane = Pane::new();
    pane.push_text("abcdefghijklmnop\n1\n2\n3\n4\n5");
    active.publish(&mut pane).unwrap();
    for event in [
        PreviewEvent::PageDown(1),
        PreviewEvent::Right(1),
        PreviewEvent::ToggleLineNumbers,
    ] {
        assert!(interface.preview_event(&mut state, event, &cx).is_none());
    }
    assert!(matches!(
        interface.preview_content(&state),
        Content::Loading
    ));
    let _ = before(&mut interface, &mut state, &cx);
    let pane = ready(interface.preview_content(&state));
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
}

#[test]
fn events_use_delivered_dimensions_and_only_redraw_on_presentation_changes() {
    let driver = driver(["abcdefghijklmnop"]);
    let previewer = RefCell::new(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 3,
        }),
    );
    let _ = before(&mut interface, &mut state, &cx);
    for event in [PreviewEvent::Right(4), PreviewEvent::PageDown(1)] {
        assert_eq!(
            interface.preview_event(&mut state, event, &cx),
            Components::PREVIEW
        );
    }
    assert_eq!(ready(interface.preview_content(&state)).offset(), (4, 3));
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 6,
        }),
    );
    let _ = interface.preview_event(&mut state, PreviewEvent::PageDown(1), &cx);
    assert_eq!(ready(interface.preview_content(&state)).offset(), (4, 9));

    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 0,
            height: 6,
        }),
    );
    assert!(
        interface
            .preview_event(&mut state, PreviewEvent::Down(1), &cx)
            .is_none()
    );
    assert_eq!(
        interface.preview_event(&mut state, PreviewEvent::SetLineNumbers(true), &cx),
        Components::PREVIEW
    );
    assert!(
        interface
            .preview_event(&mut state, PreviewEvent::SetLineNumbers(true), &cx)
            .is_none()
    );
    assert_eq!(ready(interface.preview_content(&state)).offset(), (4, 9));

    assert_eq!(previewer.borrow().requested, ["abcdefghijklmnop"]);
}

#[test]
fn revisited_cached_previews_respect_resized_height() {
    let driver = driver(["alpha", "beta"]);
    let previewer = RefCell::new(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut interface = CachedPreview::new_single(&previewer);
    let alpha = context(&driver, Some(0));
    let beta = context(&driver, Some(1));
    let mut state = interface.init(&alpha);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 20,
            height: 8,
        }),
    );
    let _ = before(&mut interface, &mut state, &alpha);
    let _ = interface.preview_event(&mut state, PreviewEvent::AlignBottom, &alpha);
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 22));
    let _ = before(&mut interface, &mut state, &beta);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 20,
            height: 12,
        }),
    );
    let _ = before(&mut interface, &mut state, &alpha);
    assert_eq!(ready(interface.preview_content(&state)).offset(), (0, 18));
    assert_eq!(previewer.borrow().requested, ["alpha", "beta"]);
}

#[test]
fn restart_and_finish_cancel_foreground_and_prefetch_before_clearing_hover() {
    use crate::session::FrameOptions;

    for restart in [false, true] {
        let mut driver = driver(["alpha", "beta", "gamma"]);
        let previewer = RefCell::new(TestPreviewer {
            defer: true,
            prefetch_limit: 2,
            prefetch_enabled: true,
            prefetch_defer: true,
            ..TestPreviewer::default()
        });
        let mut session = driver.session(CachedPreview::new_single(&previewer));
        let options = || FrameOptions::new(deadline());
        session.prepare_frame(options()).complete();
        let foreground = previewer
            .borrow_mut()
            .take_request("alpha")
            .start()
            .unwrap();
        session.prepare_frame(options()).complete();
        let prefetched = previewer.borrow_mut().take_request("beta").start().unwrap();
        session.prepare_frame(options()).complete();
        assert_eq!(previewer.borrow().prefetched, ["beta", "gamma"]);
        assert!(!foreground.is_cancelled());
        assert!(!prefetched.is_cancelled());
        if restart {
            session.events().restart();
            assert!(foreground.is_cancelled());
            assert!(prefetched.is_cancelled());
            session.events().restart();
        }
        session.finish(Exit::Quit);
        assert!(foreground.is_cancelled());
        assert!(prefetched.is_cancelled());
        let previewer = previewer.borrow();
        assert!(
            previewer
                .queued
                .iter()
                .all(crate::preview::request::QueuedRequest::is_cancelled)
        );
        assert_eq!(previewer.hovered, [Some("alpha"), None]);
        assert_eq!(previewer.cancelled_on_clear, [true]);
        assert_eq!(previewer.restarts, if restart { 2 } else { 0 });
    }
}

#[test]
fn reopening_coalesces_hidden_navigation_and_notifies_the_queued_hover_before_promotion() {
    use crate::{
        event::MatchListEvent,
        preview::{
            Preview,
            request::{ActiveRequest, Request, Response},
        },
        session::{ItemRef, tests::frame_options},
    };

    struct StartOnReveal<'a> {
        previewer: &'a RefCell<TestPreviewer>,
        active: &'a RefCell<Option<ActiveRequest<&'static str>>>,
    }

    impl Preview<&'static str> for StartOnReveal<'_> {
        fn preview(&mut self, request: Request<'_, &'static str>, timeout: Duration) -> Response {
            self.previewer.preview(request, timeout)
        }

        fn hover_changed(&mut self, item: Option<ItemRef<'_, &'static str>>) {
            self.previewer.hover_changed(item);
            if let Some(item) = item
                && !self.previewer.borrow().queued.is_empty()
            {
                let queued = self.previewer.borrow_mut().take_request(item.value());
                assert_eq!(queued.id(), item.id());
                *self.active.borrow_mut() = Some(queued.start().unwrap());
            }
        }
    }

    let mut driver = driver(["alpha", "beta"]);
    let previewer = RefCell::new(TestPreviewer {
        defer: true,
        ..TestPreviewer::default()
    });
    let active = RefCell::new(None);
    let mut session = driver.session(CachedPreview::new_single(StartOnReveal {
        previewer: &previewer,
        active: &active,
    }));
    session.prepare_frame(frame_options()).complete();
    session.events().set_active_components(Components {
        preview: false,
        ..Components::ALL
    });
    session.prepare_frame(frame_options()).complete();
    for event in [MatchListEvent::Up(1), MatchListEvent::Down(1)] {
        session.events().match_list(event);
        session.prepare_frame(frame_options()).complete();
    }
    assert!(!previewer.borrow().queued[0].is_cancelled());
    assert_eq!(previewer.borrow().requested, ["alpha"]);
    assert_eq!(previewer.borrow().hovered, [Some("alpha"), None]);

    session.events().set_active_components(Components::ALL);
    session.prepare_frame(frame_options()).complete();
    let active = active.borrow_mut().take().unwrap();
    assert!(!active.is_cancelled());
    publish(active);
    session.prepare_frame(frame_options()).complete();
    assert_eq!(previewer.borrow().requested, ["alpha"]);
    assert_eq!(
        previewer.borrow().hovered,
        [Some("alpha"), None, Some("alpha")]
    );

    session.events().set_active_components(Components {
        preview: false,
        ..Components::ALL
    });
    session.prepare_frame(frame_options()).complete();
    session.events().restart();
    drop(session);
    assert_eq!(
        previewer.borrow().hovered,
        [Some("alpha"), None, Some("alpha"), None]
    );
    assert_eq!(previewer.borrow().cancelled_on_clear, [false, true]);
}
