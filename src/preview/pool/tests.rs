use std::{
    cell::Cell,
    collections::HashSet,
    rc::Rc,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use crossbeam::channel::{Receiver, Sender, unbounded};

use super::*;
use crate::{
    Picker, PickerOptions,
    preview::{lock::Poll, request::Pending},
    render::StrRenderer,
};

mod scheduling;

const WAIT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct TestWorker {
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

impl Worker<&'static str> for TestWorker {
    fn preview(
        &mut self,
        item: &&'static str,
        pane: &mut Pane,
        is_cancelled: impl Fn() -> bool,
    ) -> bool {
        assert_eq!(pane.offset(), (0, 0));
        assert!(!pane.line_numbers());
        assert!(!pane.is_err());
        pane.set_offset(3, 7);
        pane.set_line_numbers(true);
        pane.set_err(true);
        let buffer = pane.buffer_mut();
        assert_eq!(buffer.lines().len(), 1);
        assert_eq!(buffer.line(0).unwrap().as_str(), "");
        buffer.push_str(item);
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
            return false;
        }
        assert_ne!(*item, "panic", "worker panic");
        true
    }
}

fn pool(threads: usize) -> (PoolPreviewer<&'static str>, Receiver<Started>, Sender<bool>) {
    let (started_tx, started_rx) = unbounded();
    let (release_tx, release_rx) = unbounded();
    let pool = PoolPreviewer::from_worker(
        TestWorker {
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
    picker.filter().unwrap();
    picker
}

fn submit(
    pool: &mut PoolPreviewer<&'static str>,
    picker: &Picker<&'static str, StrRenderer>,
    rank: u32,
    buffer: Buffer,
) -> Pending {
    let request = Request {
        buffer,
        epoch: 0,
        matches: picker.matches(),
        id: picker.matches().get(rank as usize).unwrap().id(),
    };
    let Response::Pending(pending) = pool.preview(request, Duration::ZERO) else {
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

#[allow(
    clippy::result_large_err,
    reason = "both outcomes return the original buffer for reuse assertions"
)]
fn completed(pending: Pending) -> Result<Pane, Buffer> {
    let mut reader = Some(pending.reader);
    let mut result = None;
    wait_until(|| {
        match reader.take().unwrap().poll() {
            Poll::Ready(buffer) => result = Some(Ok(buffer)),
            Poll::Dropped(pane) => result = Some(Err(pane.into_buffer())),
            Poll::Queued(next) | Poll::Active(next) => reader = Some(next),
        }
        result.is_some()
    });
    result.unwrap()
}

fn ready(pending: Pending) -> Buffer {
    completed(pending)
        .unwrap_or_else(|_| panic!("request was dropped"))
        .into_buffer()
}

#[test]
fn from_workers_rejects_empty_input() {
    let error = PoolPreviewer::from_workers(std::iter::empty::<TestWorker>())
        .err()
        .expect("empty worker iterator was accepted");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn synchronous_fast_path_preserves_presentation_and_clears_fallbacks_without_reallocating() {
    let picker = picker();
    for prefetch in [false, true] {
        let (pool, started, release) = pool(2);
        let mut pool = pool.with_sync_preview(|item, pane| {
            assert_eq!(pane.offset(), (0, 0));
            assert!(!pane.line_numbers());
            assert!(!pane.is_err());
            pane.set_offset(11, 23);
            pane.set_line_numbers(true);
            pane.set_err(true);
            let buffer = pane.buffer_mut();
            assert_eq!(buffer.lines().len(), 1);
            assert_eq!(buffer.line(0).unwrap().as_str(), "");
            buffer.push_text("synchronous\npreview");
            *item == "first"
        });
        let mut buffer = Buffer::new();
        buffer.push_str("reusable request storage");
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();
        buffer.clear();
        for rank in 0_u32..2 {
            let request = Request {
                buffer,
                epoch: 0,
                matches: picker.matches(),
                id: picker.matches().get(rank as usize).unwrap().id(),
            };
            let response = if prefetch {
                pool.prefetch(request, Duration::ZERO).unwrap()
            } else {
                pool.preview(request, Duration::ZERO)
            };
            let pane = if rank == 0 {
                let Response::Ready(pane) = response else {
                    panic!("synchronous preview was deferred");
                };
                assert!(started.try_recv().is_err());
                assert_eq!(pane.offset(), (11, 23));
                assert!(pane.line_numbers());
                assert!(pane.is_err());
                assert_eq!(pane.buffer().lines().len(), 2);
                assert_eq!(pane.buffer().line(0).unwrap().as_str(), "synchronous");
                assert_eq!(pane.buffer().line(1).unwrap().as_str(), "preview");
                pane
            } else {
                let Response::Pending(pending) = response else {
                    panic!("incomplete synchronous preview was published");
                };
                assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
                let pane = pending.reader.cancel_any();
                release.send(true).unwrap();
                assert_eq!(pane.offset(), (0, 0));
                assert!(!pane.line_numbers());
                assert!(!pane.is_err());
                assert_eq!(pane.buffer().lines().len(), 1);
                assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
                pane
            };
            assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
            buffer = pane.into_buffer();
            buffer.clear();
        }
    }
}

#[test]
fn from_workers_runs_non_clone_send_only_workers_from_a_borrowed_iterator() {
    struct OwnedWorker(TestWorker);

    impl Worker<&'static str> for OwnedWorker {
        fn preview(
            &mut self,
            item: &&'static str,
            pane: &mut Pane,
            is_cancelled: impl Fn() -> bool,
        ) -> bool {
            self.0.preview(item, pane, is_cancelled)
        }
    }

    let caller = thread::current().id();
    let created = Rc::new(Cell::new(0));
    let (started_tx, started_rx) = unbounded();
    let (release_tx, release_rx) = unbounded();
    let workers = (0..3).map(|_| {
        assert_eq!(thread::current().id(), caller);
        created.set(created.get() + 1);
        OwnedWorker(TestWorker {
            started: started_tx.clone(),
            release: release_rx.clone(),
            calls: Cell::new(0),
        })
    });
    let mut pool = PoolPreviewer::from_workers(workers).unwrap();
    assert_eq!(created.get(), 3);

    let picker = picker();
    let mut pending = Vec::new();
    let mut threads = HashSet::new();
    for idx in 0..3 {
        pending.push(submit(&mut pool, &picker, idx, Buffer::new()));
        let started = started_rx.recv_timeout(WAIT).unwrap();
        assert_eq!(started.call, 1);
        assert_ne!(started.thread, caller);
        assert!(threads.insert(started.thread));
    }
    for _ in 0..3 {
        release_tx.send(false).unwrap();
    }
    for (pending, expected) in pending.into_iter().zip(["first", "second", "third"]) {
        assert_eq!(ready(pending).line(0).unwrap().as_str(), expected);
    }
}

#[test]
fn from_worker_clones_on_the_caller_thread_and_moves_the_original() {
    struct CountClones {
        caller: thread::ThreadId,
        clones: Arc<AtomicUsize>,
    }

    impl Clone for CountClones {
        fn clone(&self) -> Self {
            assert_eq!(thread::current().id(), self.caller);
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                caller: self.caller,
                clones: Arc::clone(&self.clones),
            }
        }
    }

    impl Worker<&'static str> for CountClones {
        fn preview(&mut self, _: &&'static str, _: &mut Pane, _: impl Fn() -> bool) -> bool {
            unreachable!();
        }
    }

    for threads in [1, 3] {
        let clones = Arc::new(AtomicUsize::new(0));
        let worker = CountClones {
            caller: thread::current().id(),
            clones: Arc::clone(&clones),
        };
        let pool = PoolPreviewer::from_worker(worker, NonZeroUsize::new(threads).unwrap()).unwrap();
        assert_eq!(clones.load(Ordering::Relaxed), threads - 1);
        drop(pool);
    }
}

#[test]
fn default_workers_initialize_asynchronously_and_stay_on_their_threads() {
    #[derive(Debug)]
    enum Lifecycle {
        Created(thread::ThreadId),
        Previewed {
            created: thread::ThreadId,
            current: thread::ThreadId,
            calls: usize,
        },
        Dropped {
            created: thread::ThreadId,
            current: thread::ThreadId,
            calls: usize,
        },
    }

    struct Control {
        events: Sender<Lifecycle>,
        initialize: Receiver<()>,
        preview: Receiver<()>,
    }

    static CONTROL: OnceLock<Control> = OnceLock::new();

    struct DefaultWorker {
        created: thread::ThreadId,
        calls: Rc<Cell<usize>>,
    }

    impl Default for DefaultWorker {
        fn default() -> Self {
            let control = CONTROL.get().unwrap();
            let worker = Self {
                created: thread::current().id(),
                calls: Rc::new(Cell::new(0)),
            };
            control
                .events
                .send(Lifecycle::Created(worker.created))
                .unwrap();
            control.initialize.recv_timeout(WAIT).unwrap();
            worker
        }
    }

    impl Worker<&'static str> for DefaultWorker {
        fn preview(&mut self, item: &&'static str, pane: &mut Pane, _: impl Fn() -> bool) -> bool {
            let control = CONTROL.get().unwrap();
            self.calls.set(self.calls.get() + 1);
            control
                .events
                .send(Lifecycle::Previewed {
                    created: self.created,
                    current: thread::current().id(),
                    calls: self.calls.get(),
                })
                .unwrap();
            control.preview.recv_timeout(WAIT).unwrap();
            pane.buffer_mut().push_str(item);
            true
        }
    }

    impl Drop for DefaultWorker {
        fn drop(&mut self) {
            let _ = CONTROL.get().unwrap().events.send(Lifecycle::Dropped {
                created: self.created,
                current: thread::current().id(),
                calls: self.calls.get(),
            });
        }
    }

    let (events_tx, events) = unbounded();
    let (initialize, initialize_rx) = unbounded();
    let (release, release_rx) = unbounded();
    assert!(
        CONTROL
            .set(Control {
                events: events_tx,
                initialize: initialize_rx,
                preview: release_rx,
            })
            .is_ok()
    );
    let (pool_tx, pool_rx) = unbounded();
    let constructor = thread::spawn(move || {
        let pool =
            PoolPreviewer::with_default_workers::<DefaultWorker>(NonZeroUsize::new(3).unwrap());
        pool_tx.send(pool).unwrap();
    });
    let mut pool = pool_rx.recv_timeout(WAIT).unwrap().unwrap();
    let constructor_id = constructor.thread().id();
    constructor.join().unwrap();

    let mut created = HashSet::new();
    for _ in 0..3 {
        let Lifecycle::Created(id) = events.recv_timeout(WAIT).unwrap() else {
            panic!("worker ran before initialization was released");
        };
        assert_ne!(id, constructor_id);
        assert_ne!(id, thread::current().id());
        assert!(created.insert(id));
    }
    let picker = picker();
    let mut pending = vec![submit(&mut pool, &picker, 0, Buffer::new())];
    assert!(events.try_recv().is_err());
    for _ in 0..3 {
        initialize.send(()).unwrap();
    }
    let mut previewed = HashSet::new();
    for idx in 0..3 {
        if idx != 0 {
            pending.push(submit(&mut pool, &picker, idx, Buffer::new()));
        }
        let Lifecycle::Previewed {
            created,
            current,
            calls,
        } = events.recv_timeout(WAIT).unwrap()
        else {
            panic!("expected a preview");
        };
        assert_eq!(created, current);
        assert_eq!(calls, 1);
        assert!(previewed.insert(current));
    }
    assert_eq!(previewed, created);
    for _ in 0..3 {
        release.send(()).unwrap();
    }
    for (pending, expected) in pending.into_iter().zip(["first", "second", "third"]) {
        assert_eq!(ready(pending).line(0).unwrap().as_str(), expected);
    }

    let pending = submit(&mut pool, &picker, 3, Buffer::new());
    let Lifecycle::Previewed {
        created: id,
        current,
        calls,
    } = events.recv_timeout(WAIT).unwrap()
    else {
        panic!("expected a reused worker");
    };
    assert!(created.contains(&id));
    assert_eq!(id, current);
    assert_eq!(calls, 2);
    release.send(()).unwrap();
    assert_eq!(ready(pending).line(0).unwrap().as_str(), "fourth");
    drop(pool);

    let mut dropped = HashSet::new();
    let mut total_calls = 0;
    for _ in 0..3 {
        let Lifecycle::Dropped {
            created,
            current,
            calls,
        } = events.recv_timeout(WAIT).unwrap()
        else {
            panic!("expected worker destruction");
        };
        assert_eq!(created, current);
        assert!(dropped.insert(current));
        total_calls += calls;
    }
    assert_eq!(dropped, created);
    assert_eq!(total_calls, 4);
    assert!(events.try_recv().is_err());
}

#[test]
fn default_worker_panic_releases_requests_and_propagates_on_submission() {
    static INITIALIZE: OnceLock<Receiver<()>> = OnceLock::new();
    struct PanickingDefault;

    impl Default for PanickingDefault {
        fn default() -> Self {
            INITIALIZE.get().unwrap().recv_timeout(WAIT).unwrap();
            panic!("default worker panic");
        }
    }

    impl Worker<&'static str> for PanickingDefault {
        fn preview(&mut self, _: &&'static str, _: &mut Pane, _: impl Fn() -> bool) -> bool {
            unreachable!();
        }
    }

    let (initialize, initialize_rx) = unbounded();
    INITIALIZE.set(initialize_rx).unwrap();
    let mut pool =
        PoolPreviewer::with_default_workers::<PanickingDefault>(NonZeroUsize::new(1).unwrap())
            .unwrap();
    let picker = picker();
    let pending = submit(&mut pool, &picker, 0, Buffer::new());
    initialize.send(()).unwrap();

    assert!(completed(pending).is_err());
    assert!(pool.is_closed());
    let panic = catch_unwind(AssertUnwindSafe(|| {
        submit(&mut pool, &picker, 1, Buffer::new())
    }))
    .err()
    .expect("initialization panic was not propagated");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"default worker panic"));
    assert!(pool.is_closed());
}

#[test]
fn shutdown_waits_for_default_initialization() {
    static INITIALIZE: OnceLock<Receiver<()>> = OnceLock::new();
    struct WaitingDefault;

    impl Default for WaitingDefault {
        fn default() -> Self {
            INITIALIZE.get().unwrap().recv_timeout(WAIT).unwrap();
            Self
        }
    }

    impl Worker<&'static str> for WaitingDefault {
        fn preview(&mut self, _: &&'static str, _: &mut Pane, _: impl Fn() -> bool) -> bool {
            unreachable!();
        }
    }

    let (initialize, initialize_rx) = unbounded();
    INITIALIZE.set(initialize_rx).unwrap();
    let pool = PoolPreviewer::with_default_workers::<WaitingDefault>(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let shared = Arc::clone(&pool.pool.shared);
    let shutdown = thread::spawn(move || drop(pool));
    wait_until(|| shared.state.lock().jobs.is_none());
    assert!(!shutdown.is_finished());
    initialize.send(()).unwrap();
    shutdown.join().unwrap();
}

#[test]
fn incomplete_previews_are_dropped_and_scratch_buffers_are_cleared_and_reused() {
    struct IncompleteWorker(Option<usize>);

    impl Worker<&'static str> for IncompleteWorker {
        fn preview(
            &mut self,
            item: &&'static str,
            pane: &mut Pane,
            is_cancelled: impl Fn() -> bool,
        ) -> bool {
            assert!(!is_cancelled());
            assert_eq!(pane.offset(), (0, 0));
            assert!(!pane.line_numbers());
            assert!(!pane.is_err());
            if self.0.is_none() {
                pane.set_offset(13, 31);
                pane.set_line_numbers(true);
                pane.set_err(true);
            }
            let buffer = pane.buffer_mut();
            assert_eq!(buffer.lines().len(), 1);
            assert_eq!(buffer.line(0).unwrap().as_str(), "");
            if let Some(allocation) = self.0 {
                buffer.push_str(item);
                assert_eq!(
                    buffer.line(0).unwrap().as_str().as_ptr() as usize,
                    allocation
                );
                true
            } else {
                buffer.push_text("incomplete\npreview");
                self.0 = Some(buffer.line(0).unwrap().as_str().as_ptr() as usize);
                false
            }
        }
    }

    let picker = picker();
    let mut pool = PoolPreviewer::from_workers([IncompleteWorker(None)]).unwrap();
    let mut buffer = Buffer::new();
    buffer.push_str("reusable request buffer");
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    buffer.clear();
    let incomplete = submit(&mut pool, &picker, 0, buffer);
    let buffer = completed(incomplete).expect_err("incomplete preview was published");
    assert_eq!(buffer.line(0).unwrap().as_str(), "");
    assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);

    let retry = submit(&mut pool, &picker, 0, buffer);
    let pane = completed(retry).unwrap();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    let buffer = pane.buffer();
    assert_eq!(buffer.lines().len(), 1);
    assert_eq!(buffer.line(0).unwrap().as_str(), "first");
}

#[test]
fn reuses_worker_state_and_buffers_after_cancellation_and_publication() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let first = submit(&mut pool, &picker, 0, Buffer::new());
    let first_started = started.recv_timeout(WAIT).unwrap();
    first.reader.cancel_any();

    let mut recycled = Buffer::new();
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
    let pane = completed(second).unwrap();
    assert!(pane.is_err());
    assert_eq!(pane.offset(), (3, 7));
    assert!(pane.line_numbers());

    let third = submit(&mut pool, &picker, 2, Buffer::new());
    let third_started = started.recv_timeout(WAIT).unwrap();
    assert_eq!(third_started.buffer, recycled_ptr);
    assert_eq!(third_started.call, 3);
    release.send(false).unwrap();
    assert_eq!(ready(third).line(0).unwrap().as_str(), "third");
}

#[test]
fn bounds_cancelled_backlog_and_drops_queued_work_on_shutdown() {
    let picker = picker();
    let (mut pool, started, release) = pool(2);
    let active = [0, 1].map(|idx| {
        let pending = submit(&mut pool, &picker, idx, Buffer::new());
        started.recv_timeout(WAIT).unwrap();
        pending
    });

    let pending = Vec::from_fn(64, |_| submit(&mut pool, &picker, 1, Buffer::new()));
    for _ in 0..1024 {
        submit(&mut pool, &picker, 2, Buffer::new())
            .reader
            .cancel_any();
    }
    {
        let state = pool.pool.shared.state.lock();
        let jobs = state.jobs.as_ref().unwrap();
        assert_eq!(
            jobs.urgent
                .iter()
                .chain(&jobs.background)
                .filter(|job| !job.is_cancelled())
                .count(),
            2
        );
        assert_eq!(jobs.background.len(), 2);
    }

    let shared = Arc::clone(&pool.pool.shared);
    let shutdown = thread::spawn(move || drop(pool));
    wait_until(|| shared.state.lock().jobs.is_none());
    assert!(!shutdown.is_finished());
    for _ in 0..2 {
        release.send(false).unwrap();
    }
    shutdown.join().unwrap();
    for (pending, expected) in active.into_iter().zip(["first", "second"]) {
        assert_eq!(ready(pending).line(0).unwrap().as_str(), expected);
    }
    for pending in pending {
        assert!(completed(pending).is_err());
    }
    assert!(started.try_recv().is_err());
}

#[test]
fn worker_panic_releases_requests_and_propagates_on_submission() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    assert!(!pool.is_closed());
    let panicking = submit(&mut pool, &picker, 4, Buffer::new());
    started.recv_timeout(WAIT).unwrap();
    let background = submit(&mut pool, &picker, 1, Buffer::new());
    let queued = submit(&mut pool, &picker, 0, Buffer::new());
    release.send(false).unwrap();

    assert!(completed(panicking).is_err());
    assert!(completed(background).is_err());
    assert!(completed(queued).is_err());
    assert!(pool.is_closed());
    let panic = catch_unwind(AssertUnwindSafe(|| {
        submit(&mut pool, &picker, 1, Buffer::new())
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
