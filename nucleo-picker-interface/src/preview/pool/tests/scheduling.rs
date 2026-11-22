use super::*;

#[test]
fn background_work_reserves_a_worker_for_current_demand() {
    let picker = picker();
    let (mut pool, started, release) = pool(2);
    let background = prefetch(&mut pool, &picker, 0);
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "first");
    let queued = prefetch(&mut pool, &picker, 1);
    let foreground = submit(&mut pool, &picker, 2);
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "third");
    for _ in 0..2 {
        release.send(Finish::Publish).unwrap();
    }
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
    release.send(Finish::Publish).unwrap();
    for (pending, expected) in [
        (background, "first"),
        (queued, "second"),
        (foreground, "third"),
    ] {
        assert_ready(pending, expected);
    }
}

#[test]
fn hover_changes_demote_queued_work_even_when_the_new_preview_is_synchronous() {
    for (hovered, synchronous) in [(None, false), (Some(0), false), (Some(3), true)] {
        let picker = picker();
        let (pool, started, release) = pool(2);
        let mut pool = pool.with_sync_preview(|item, pane| {
            if synchronous && *item == "fourth" {
                pane.push_text(item);
                true
            } else {
                false
            }
        });
        let active = [0, 1].map(|rank| {
            let pending = submit(&mut pool, &picker, rank);
            started.recv_timeout(WAIT).unwrap();
            pending
        });
        let outdated = submit(&mut pool, &picker, 2);
        pool.hover_changed(hovered.and_then(|rank| picker.matches().get(rank)));
        if synchronous {
            let Response::Ready(pane) = pool.preview(request(&picker, 3), Duration::ZERO) else {
                panic!("synchronous preview was deferred");
            };
            assert_eq!(pane.buffer().line(0).unwrap().as_str(), "fourth");
        }
        release.send(Finish::Publish).unwrap();
        let early = started.recv_timeout(QUIET).ok();
        release.send(Finish::Publish).unwrap();
        let next = early
            .as_ref()
            .map(|started| started.item)
            .unwrap_or_else(|| started.recv_timeout(WAIT).unwrap().item);
        release.send(Finish::Publish).unwrap();
        assert_eq!(next, "third");
        for (pending, expected) in active.into_iter().zip(["first", "second"]) {
            assert_ready(pending, expected);
        }
        assert_ready(outdated, "third");
        assert!(
            early.is_none(),
            "outdated work consumed the reserved worker"
        );
    }
}

#[test]
fn disabled_prefetch_does_not_prevent_foreground_work() {
    for threads in [1, 2] {
        let picker = picker();
        let (pool, started, release) = pool(threads);
        let mut pool = if threads == 1 {
            pool
        } else {
            pool.with_prefetch_limit(0)
        };
        assert!(pool.prefetch(request(&picker, 0), Duration::ZERO).is_err());
        let pending = submit(&mut pool, &picker, 1);
        assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
        release.send(Finish::Publish).unwrap();
        assert_ready(pending, "second");
    }
}

#[test]
fn a_single_worker_skips_outdated_demand() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let first = submit(&mut pool, &picker, 0);
    started.recv_timeout(WAIT).unwrap();
    let outdated = submit(&mut pool, &picker, 1);
    let current = submit(&mut pool, &picker, 2);
    release.send(Finish::Publish).unwrap();
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "third");
    release.send(Finish::Publish).unwrap();
    assert_ready(first, "first");
    assert_ready(current, "third");
    drop(pool);
    assert!(completed(outdated).is_none());
    assert!(started.try_recv().is_err());
}

#[test]
fn current_demand_precedes_background_work_and_cancelled_jobs_are_skipped() {
    for cancel_hovered in [false, true] {
        let picker = picker();
        let (mut pool, started, release) = pool(2);
        let active = [0, 1].map(|rank| {
            let pending = submit(&mut pool, &picker, rank);
            started.recv_timeout(WAIT).unwrap();
            pending
        });
        prefetch(&mut pool, &picker, 3).reader.cancel_any();
        let background = prefetch(&mut pool, &picker, 2);
        let mut hovered = Some(submit(&mut pool, &picker, 0));
        pool.hover_changed(picker.matches().get(0));
        if cancel_hovered {
            hovered.take().unwrap().reader.cancel_any();
        }
        for _ in 0..2 {
            release.send(Finish::Publish).unwrap();
        }
        let expected = if cancel_hovered {
            &["third"][..]
        } else {
            &["first", "third"][..]
        };
        for &expected in expected {
            assert_eq!(started.recv_timeout(WAIT).unwrap().item, expected);
            release.send(Finish::Publish).unwrap();
        }
        for (pending, expected) in active
            .into_iter()
            .zip(["first", "second"])
            .chain([(background, "third")])
            .chain(hovered.map(|pending| (pending, "first")))
        {
            assert_ready(pending, expected);
        }
        drop(pool);
        assert!(started.try_recv().is_err());
    }
}
