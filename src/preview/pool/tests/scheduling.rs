use super::*;
use crate::{
    component::PreviewComponent,
    preview::pane::{PreviewPane, tests::support},
};

const QUIET: Duration = Duration::from_millis(50);

fn prefetch(
    pool: &mut PoolPreviewer<&'static str>,
    picker: &Picker<&'static str, StrRenderer>,
    idx: u32,
) -> PendingPreview {
    let request = PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 0,
        snapshot: picker.core.engine.snapshot(),
        id: picker.core.engine.item_id(idx),
    };
    let Ok(PreviewResponse::Pending(pending)) = pool.prefetch(request, Duration::ZERO) else {
        panic!("pool did not accept prefetch");
    };
    pending
}

#[test]
fn background_work_preserves_capacity_for_the_next_highlighted_request() {
    for foreground in [false, true] {
        let picker = support::picker(["first", "second", "third", "fourth"]);
        let (pool, started, release) = pool(2);
        let shared = Arc::clone(&pool.pool.shared);
        let mut pane = PreviewPane::new(
            &PickerOptions::new().preview_config,
            &crate::PickerChars::new(),
            pool.with_prefetch_limit(2),
        );
        pane.run_frame(Some(0), &picker.core.engine, support::deadline());
        assert_eq!(started.recv_timeout(WAIT).unwrap().item, "first");
        if !foreground {
            release.send(false).unwrap();
            wait_until(|| {
                pane.run_frame(Some(0), &picker.core.engine, support::deadline());
                match started.try_recv() {
                    Ok(started) => {
                        assert_eq!(started.item, "second");
                        true
                    }
                    Err(_) => false,
                }
            });
        }
        pane.run_frame(Some(0), &picker.core.engine, support::deadline());
        let (rank, expected) = if foreground {
            (2, "third")
        } else {
            (3, "fourth")
        };
        pane.run_frame(Some(rank), &picker.core.engine, Instant::now());
        let next = started.recv_timeout(WAIT).unwrap();
        for _ in 0..2 {
            release.send(false).unwrap();
        }
        wait_until(|| shared.state.lock().idle_workers == 2);
        pane.run_frame(Some(rank), &picker.core.engine, Instant::now());
        assert_eq!(next.item, expected);
        assert_eq!(pane.contents(), [expected]);
    }
}

#[test]
fn reopening_the_pane_promotes_the_queued_selection() {
    for prefetch in [0, 4] {
        let picker = support::picker(["first", "second"]);
        let (pool, started, release) = pool(1);
        let shared = Arc::clone(&pool.pool.shared);
        let mut pane = PreviewPane::new(
            &PickerOptions::new().preview_config,
            &crate::PickerChars::new(),
            pool.with_prefetch_limit(prefetch),
        );
        pane.run_frame(Some(0), &picker.core.engine, support::deadline());
        assert_eq!(started.recv_timeout(WAIT).unwrap().item, "first");
        pane.run_frame(Some(1), &picker.core.engine, support::deadline());
        pane.hide();
        pane.run_frame(Some(1), &picker.core.engine, support::deadline());

        release.send(false).unwrap();
        let next = started
            .recv_timeout(WAIT)
            .expect("reopened preview remained queued despite an idle worker");
        release.send(false).unwrap();
        wait_until(|| shared.state.lock().idle_workers == 1);
        pane.run_frame(Some(1), &picker.core.engine, support::deadline());
        assert_eq!(next.item, "second");
        assert_eq!(pane.contents(), ["second"]);
    }
}

#[test]
fn focus_changes_without_submission_demote_the_urgent_request() {
    for focused in [None, Some(0)] {
        let picker = picker();
        let (mut pool, started, release) = pool(2);
        let first = submit(&mut pool, &picker, 0, PreviewBuffer::new());
        started.recv_timeout(WAIT).unwrap();
        let second = submit(&mut pool, &picker, 1, PreviewBuffer::new());
        started.recv_timeout(WAIT).unwrap();

        let outdated = submit(&mut pool, &picker, 2, PreviewBuffer::new());
        pool.focus_changed(focused.and_then(|idx| picker.core.engine.item_ref(idx)));
        let background = prefetch(&mut pool, &picker, 3);
        release.send(false).unwrap();
        let early = started.recv_timeout(QUIET).ok();
        release.send(false).unwrap();
        let next = early
            .as_ref()
            .map(|started| started.item)
            .unwrap_or_else(|| started.recv_timeout(WAIT).unwrap().item);
        for _ in 0..2 {
            release.send(false).unwrap();
        }
        for pending in [first, second, outdated, background] {
            ready(pending);
        }

        assert!(
            early.is_none(),
            "outdated work consumed the reserved worker"
        );
        assert_eq!(next, "fourth");
    }
}

#[test]
fn synchronous_previews_demote_queued_work_and_preserve_the_reserved_worker() {
    let picker = support::picker(["first", "second", "queued", "sync"]);
    let (pool, started, release) = pool(2);
    let shared = Arc::clone(&pool.pool.shared);
    let mut pane = support::preview_pane(pool.with_prefetch_limit(0).with_sync_preview(
        |item, buffer| {
            if *item == "sync" {
                buffer.push_str(item);
                true
            } else {
                false
            }
        },
    ));
    for rank in 0..2 {
        pane.run_frame(Some(rank), &picker.core.engine, support::deadline());
        started.recv_timeout(WAIT).unwrap();
    }
    pane.run_frame(Some(2), &picker.core.engine, support::deadline());
    pane.run_frame(Some(3), &picker.core.engine, support::deadline());
    assert_eq!(pane.contents(), ["sync"]);

    release.send(false).unwrap();
    let early = started.recv_timeout(QUIET).ok();
    release.send(false).unwrap();
    let next = early
        .as_ref()
        .map(|started| started.item)
        .unwrap_or_else(|| started.recv_timeout(WAIT).unwrap().item);
    release.send(false).unwrap();
    wait_until(|| shared.state.lock().idle_workers == 2);

    assert!(
        early.is_none(),
        "outdated work consumed the reserved worker"
    );
    assert_eq!(next, "queued");
}

#[test]
fn a_single_worker_only_runs_current_demand_and_preserves_declined_buffers() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    assert_eq!(pool.prefetch_limit(), 0);
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("reusable allocation");
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    buffer.clear();
    let response = pool.prefetch(
        PreviewRequest {
            buffer,
            epoch: 0,
            snapshot: picker.core.engine.snapshot(),
            id: picker.core.engine.item_id(0),
        },
        Duration::ZERO,
    );
    let Err(buffer) = response else {
        panic!("single-worker pool accepted speculative work");
    };
    assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);

    let first = submit(&mut pool, &picker, 0, buffer);
    started.recv_timeout(WAIT).unwrap();
    let outdated = submit(&mut pool, &picker, 1, PreviewBuffer::new());
    let highlighted = submit(&mut pool, &picker, 2, PreviewBuffer::new());
    release.send(false).unwrap();
    let next = started.recv_timeout(WAIT).unwrap();
    release.send(false).unwrap();
    ready(first);
    ready(highlighted);
    let early = started.recv_timeout(QUIET).ok();
    release.send(false).unwrap();
    drop(pool);

    assert_eq!(next.item, "third");
    assert!(early.is_none());
    assert!(completed(outdated).is_err());
}

#[test]
fn disabling_pool_prefetch_preserves_buffers_and_foreground_work() {
    let picker = picker();
    let (pool, started, release) = pool(2);
    let mut pool = pool.with_prefetch_limit(0);
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("reusable allocation");
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    buffer.clear();
    let Err(buffer) = pool.prefetch(
        PreviewRequest {
            buffer,
            epoch: 0,
            snapshot: picker.core.engine.snapshot(),
            id: picker.core.engine.item_id(0),
        },
        Duration::ZERO,
    ) else {
        panic!("disabled pool accepted speculative work");
    };
    assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);
    let pending = submit(&mut pool, &picker, 1, buffer);
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
    release.send(false).unwrap();
    ready(pending);
}

#[test]
fn current_demand_precedes_lifo_background_work_and_cancelled_jobs_are_skipped() {
    for cancel_highlighted in [false, true] {
        let picker = picker();
        let (mut pool, started, release) = pool(2);
        let first = submit(&mut pool, &picker, 0, PreviewBuffer::new());
        started.recv_timeout(WAIT).unwrap();
        let second = submit(&mut pool, &picker, 1, PreviewBuffer::new());
        started.recv_timeout(WAIT).unwrap();

        let outdated = submit(&mut pool, &picker, 2, PreviewBuffer::new());
        let older_prefetch = prefetch(&mut pool, &picker, 3);
        let mut highlighted = Some(submit(&mut pool, &picker, 0, PreviewBuffer::new()));
        pool.focus_changed(picker.core.engine.item_ref(0));
        prefetch(&mut pool, &picker, 3).reader.cancel_any();
        let newer_prefetch = prefetch(&mut pool, &picker, 1);
        assert!(completed(older_prefetch).is_err());
        if cancel_highlighted {
            highlighted.take().unwrap().reader.cancel_any();
        }
        for _ in 0..2 {
            release.send(false).unwrap();
        }
        let expected = if cancel_highlighted {
            &["second", "third"][..]
        } else {
            &["first", "second", "third"][..]
        };
        let mut order = Vec::new();
        for _ in expected {
            order.push(started.recv_timeout(WAIT).unwrap().item);
            release.send(false).unwrap();
        }
        for pending in [first, second, outdated, newer_prefetch]
            .into_iter()
            .chain(highlighted)
        {
            ready(pending);
        }
        assert_eq!(order, expected);
    }
}

#[test]
fn initializing_workers_do_not_count_as_spare_capacity() {
    let picker = picker();
    let (started_tx, started) = unbounded();
    let (release, release_rx) = unbounded();
    let (initialize, initialize_rx) = unbounded();
    let worker = Worker {
        started: started_tx,
        release: release_rx,
        calls: Cell::new(0),
    };
    let delayed = worker.clone();
    let mut pool = PoolPreviewer::new(2);
    pool.pool.spawn(move || worker).unwrap();
    pool.pool
        .spawn(move || {
            initialize_rx.recv_timeout(WAIT).unwrap();
            delayed
        })
        .unwrap();

    let background = prefetch(&mut pool, &picker, 0);
    let early = started.recv_timeout(QUIET).ok();
    let highlighted = submit(&mut pool, &picker, 1, PreviewBuffer::new());
    if early.is_some() {
        release.send(false).unwrap();
    }
    let next = started.recv_timeout(WAIT).unwrap();
    release.send(false).unwrap();
    ready(highlighted);
    initialize.send(()).unwrap();
    release.send(false).unwrap();
    ready(background);

    assert!(early.is_none(), "initializing worker was treated as idle");
    assert_eq!(next.item, "second");
}
