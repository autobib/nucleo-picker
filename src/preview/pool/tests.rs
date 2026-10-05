use std::{cell::Cell, collections::HashSet, time::Instant};

use crossbeam::channel::{Receiver, Sender, unbounded};

use super::*;
use crate::{
    Picker, PickerOptions,
    preview::{PendingPreview, lock::Poll},
    render::StrRenderer,
};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Worker {
    started: Sender<Started>,
    release: Receiver<bool>,
    calls: Cell<usize>,
}

struct Started {
    item: &'static str,
    call: usize,
    thread: thread::ThreadId,
    buffer: usize,
}

impl PreviewWorker<&'static str> for Worker {
    fn preview(
        &mut self,
        item: &&'static str,
        buffer: &mut PreviewBuffer,
        is_cancelled: impl Fn() -> bool,
    ) {
        assert_eq!(buffer.lines().len(), 1);
        assert_eq!(buffer.line(0).unwrap().as_str(), "");
        assert!(!buffer.is_err());
        buffer.push_str(item);
        buffer.set_err(true);
        self.calls.set(self.calls.get() + 1);
        self.started
            .send(Started {
                item,
                call: self.calls.get(),
                thread: thread::current().id(),
                buffer: buffer.line(0).unwrap().as_str().as_ptr() as usize,
            })
            .unwrap();
        let expected_cancelled = self.release.recv_timeout(WAIT).unwrap();
        let cancelled = is_cancelled();
        assert_eq!(cancelled, expected_cancelled);
        if cancelled {
            return;
        }
        assert_ne!(*item, "panic", "worker panic");
    }
}

fn pool(threads: usize) -> (PoolPreviewer<&'static str>, Receiver<Started>, Sender<bool>) {
    let (started_tx, started_rx) = unbounded();
    let (release_tx, release_rx) = unbounded();
    let pool = PoolPreviewer::new(
        Worker {
            started: started_tx,
            release: release_rx,
            calls: Cell::new(0),
        },
        NonZeroUsize::new(threads).unwrap(),
    )
    .unwrap();
    (pool, started_rx, release_tx)
}

fn picker() -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new().picker(StrRenderer);
    picker.push_batch(["first", "second", "third", "fourth", "panic"]);
    wait_until(|| !picker.engine.update(5).matching);
    picker
}

fn submit(
    pool: &mut PoolPreviewer<&'static str>,
    picker: &Picker<&'static str, StrRenderer>,
    idx: u32,
    buffer: PreviewBuffer,
) -> PendingPreview {
    let request = PreviewRequest {
        buffer,
        epoch: 0,
        snapshot: picker.engine.snapshot(),
        idx,
    };
    let PreviewResponse::Pending(pending) = pool
        .preview(request.item(), request, Duration::ZERO)
        .unwrap()
    else {
        panic!("pool did not defer request");
    };
    pending
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < WAIT, "timed out");
        thread::yield_now();
    }
}

fn completed(pending: PendingPreview) -> Result<PreviewBuffer, PreviewBuffer> {
    let mut reader = Some(pending.reader);
    let mut result = None;
    wait_until(|| {
        match reader.take().unwrap().poll() {
            Poll::Ready(buffer) => result = Some(Ok(buffer)),
            Poll::Dropped(buffer) => result = Some(Err(buffer)),
            Poll::Queued(next) | Poll::Active(next) => reader = Some(next),
        }
        result.is_some()
    });
    result.unwrap()
}

fn ready(pending: PendingPreview) -> PreviewBuffer {
    completed(pending).unwrap_or_else(|_| panic!("request was dropped"))
}

#[test]
fn dispatches_newest_first_and_skips_cancelled_requests() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let first = submit(&mut pool, &picker, 0, PreviewBuffer::new());
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "first");

    let second = submit(&mut pool, &picker, 1, PreviewBuffer::new());
    let cancelled = submit(&mut pool, &picker, 2, PreviewBuffer::new());
    let fourth = submit(&mut pool, &picker, 3, PreviewBuffer::new());
    cancelled.reader.cancel_any();

    release.send(false).unwrap();
    let next = started.recv_timeout(WAIT).unwrap();
    assert_eq!((next.item, next.call), ("fourth", 2));
    release.send(false).unwrap();
    let next = started.recv_timeout(WAIT).unwrap();
    assert_eq!((next.item, next.call), ("second", 3));
    release.send(false).unwrap();

    for (pending, expected) in [(first, "first"), (second, "second"), (fourth, "fourth")] {
        assert_eq!(ready(pending).line(0).unwrap().as_str(), expected);
    }
}

#[test]
fn reuses_worker_state_and_buffers_after_cancellation_and_publication() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let first = submit(&mut pool, &picker, 0, PreviewBuffer::new());
    let first_started = started.recv_timeout(WAIT).unwrap();
    first.reader.cancel_any();

    let mut recycled = PreviewBuffer::new();
    recycled.push_str("allocated storage for the next request");
    let recycled_ptr = recycled.line(0).unwrap().as_str().as_ptr() as usize;
    recycled.clear();
    let second = submit(&mut pool, &picker, 0, recycled);
    release.send(true).unwrap();
    let second_started = started.recv_timeout(WAIT).unwrap();
    assert_eq!(second_started.buffer, first_started.buffer);
    assert_eq!(second_started.thread, first_started.thread);
    assert_eq!(second_started.call, 2);
    release.send(false).unwrap();
    assert!(ready(second).is_err());

    let third = submit(&mut pool, &picker, 2, PreviewBuffer::new());
    let third_started = started.recv_timeout(WAIT).unwrap();
    assert_eq!(third_started.buffer, recycled_ptr);
    assert_eq!(third_started.call, 3);
    release.send(false).unwrap();
    assert_eq!(ready(third).line(0).unwrap().as_str(), "third");
}

#[test]
fn bounds_cancelled_backlog_and_drops_queued_work_on_shutdown() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let active = submit(&mut pool, &picker, 0, PreviewBuffer::new());
    started.recv_timeout(WAIT).unwrap();

    let pending: Vec<_> = (0..64)
        .map(|_| submit(&mut pool, &picker, 1, PreviewBuffer::new()))
        .collect();
    for _ in 0..1024 {
        submit(&mut pool, &picker, 2, PreviewBuffer::new())
            .reader
            .cancel_any();
    }
    {
        let state = pool.shared.state.lock();
        let jobs = state.jobs.as_ref().unwrap();
        assert_eq!(
            jobs.iter().filter(|job| !job.is_cancelled()).count(),
            pending.len()
        );
        assert!(jobs.len() <= 2 * (pending.len() + MIN_PRUNE_AT));
    }

    let shared = Arc::clone(&pool.shared);
    let shutdown = thread::spawn(move || drop(pool));
    wait_until(|| shared.state.lock().jobs.is_none());
    assert!(!shutdown.is_finished());
    release.send(false).unwrap();
    shutdown.join().unwrap();
    assert_eq!(ready(active).line(0).unwrap().as_str(), "first");
    for pending in pending {
        assert!(completed(pending).is_err());
    }
    assert!(started.try_recv().is_err());
}

#[test]
fn runs_send_only_workers_concurrently_on_the_requested_threads() {
    let picker = picker();
    let (mut pool, started, release) = pool(3);
    let pending: Vec<_> = (0..3)
        .map(|idx| submit(&mut pool, &picker, idx, PreviewBuffer::new()))
        .collect();
    let mut threads = HashSet::new();
    for _ in 0..3 {
        let started = started.recv_timeout(WAIT).unwrap();
        assert_eq!(started.call, 1);
        assert_ne!(started.thread, thread::current().id());
        threads.insert(started.thread);
    }
    assert_eq!(threads.len(), 3);
    for _ in 0..3 {
        release.send(false).unwrap();
    }
    drop(pool);
    for pending in pending {
        assert!(completed(pending).is_ok());
    }
}

#[test]
fn worker_panic_releases_requests_and_propagates_on_submission() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    assert!(!pool.is_closed());
    let panicking = submit(&mut pool, &picker, 4, PreviewBuffer::new());
    started.recv_timeout(WAIT).unwrap();
    let queued = submit(&mut pool, &picker, 0, PreviewBuffer::new());
    release.send(false).unwrap();

    assert!(completed(panicking).is_err());
    assert!(completed(queued).is_err());
    assert!(pool.is_closed());
    let panic = catch_unwind(AssertUnwindSafe(|| {
        submit(&mut pool, &picker, 1, PreviewBuffer::new())
    }))
    .err()
    .expect("worker panic was not propagated");
    assert!(
        panic
            .downcast_ref::<String>()
            .unwrap()
            .contains("worker panic")
    );
    assert!(pool.is_closed());
}
