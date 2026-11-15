use std::{cell::Cell, collections::HashSet, rc::Rc, sync::OnceLock, time::Instant};

use crossbeam::channel::{Receiver, Sender, unbounded};

use super::*;
use crate::{
    Picker,
    preview::{
        lock::Poll,
        request::{Pending, tests::request},
    },
    render::StrRenderer,
};

mod scheduling;

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug)]
enum Finish {
    Publish,
    Abandon,
    Cancelled,
    Panic,
}

#[derive(Clone)]
struct TestWorker {
    started: Sender<Started>,
    release: Receiver<Finish>,
    calls: Cell<usize>,
}

struct Started {
    item: &'static str,
    call: usize,
    thread: thread::ThreadId,
}

fn assert_empty(pane: &Pane) {
    assert!(pane.buffer().lines().map(|line| line.as_str()).eq([""]));
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
}

impl Worker<&'static str> for TestWorker {
    fn preview(
        &mut self,
        item: &&'static str,
        pane: &mut Pane,
        is_cancelled: impl Fn() -> bool,
    ) -> bool {
        assert_empty(pane);
        pane.set_offset(3, 7);
        pane.set_line_numbers(true);
        pane.set_err(true);
        pane.push_text(item);
        self.calls.set(self.calls.get() + 1);
        self.started
            .send(Started {
                item,
                call: self.calls.get(),
                thread: thread::current().id(),
            })
            .unwrap();
        let finish = self.release.recv_timeout(WAIT).unwrap();
        assert_eq!(is_cancelled(), matches!(finish, Finish::Cancelled));
        match finish {
            Finish::Publish => true,
            Finish::Abandon | Finish::Cancelled => false,
            Finish::Panic => panic!("worker panic"),
        }
    }
}

fn pool(
    threads: usize,
) -> (
    PoolPreviewer<&'static str>,
    Receiver<Started>,
    Sender<Finish>,
) {
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
    let mut picker = Picker::new(StrRenderer);
    picker.push_batch(["first", "second", "third", "fourth"]);
    picker.filter().unwrap();
    picker
}

fn submit<F>(
    pool: &mut PoolPreviewer<&'static str, F>,
    picker: &Picker<&'static str, StrRenderer>,
    rank: usize,
) -> Pending
where
    F: FnMut(&&'static str, &mut Pane) -> bool,
{
    let Response::Pending(pending) = pool.preview(request(picker, rank), Duration::ZERO) else {
        panic!("pool did not defer request");
    };
    pending
}

fn prefetch<F>(
    pool: &mut PoolPreviewer<&'static str, F>,
    picker: &Picker<&'static str, StrRenderer>,
    rank: usize,
) -> Pending
where
    F: FnMut(&&'static str, &mut Pane) -> bool,
{
    let Ok(Response::Pending(pending)) = pool.prefetch(request(picker, rank), Duration::ZERO)
    else {
        panic!("pool did not accept prefetch");
    };
    pending
}

fn completed(pending: Pending) -> Option<Pane> {
    let start = Instant::now();
    let mut reader = pending.reader;
    loop {
        reader = match reader.poll() {
            Poll::Ready(pane) => return Some(pane),
            Poll::Dropped(_) => return None,
            Poll::Queued(reader) | Poll::Active(reader) => reader,
        };
        assert!(start.elapsed() < WAIT, "request timed out");
        thread::yield_now();
    }
}

fn assert_ready(pending: Pending, expected: &str) {
    let pane = completed(pending).expect("request was dropped");
    assert!(
        pane.buffer()
            .lines()
            .map(|line| line.as_str())
            .eq([expected])
    );
}

#[test]
fn synchronous_previews_preserve_presentation_and_clear_declined_work() {
    let picker = picker();
    for prefetch in [false, true] {
        let (pool, started, release) = pool(2);
        let mut pool = pool.with_sync_preview(|item, pane| {
            assert_empty(pane);
            pane.set_offset(11, 23);
            pane.set_line_numbers(true);
            pane.set_err(true);
            pane.push_text("synchronous\npreview");
            *item == "first"
        });
        for rank in 0..2 {
            let request = request(&picker, rank);
            let response = if prefetch {
                pool.prefetch(request, Duration::ZERO).unwrap()
            } else {
                pool.preview(request, Duration::ZERO)
            };
            if rank == 0 {
                let Response::Ready(pane) = response else {
                    panic!("synchronous preview was deferred");
                };
                assert!(started.try_recv().is_err());
                assert!(
                    pane.buffer()
                        .lines()
                        .map(|line| line.as_str())
                        .eq(["synchronous", "preview"])
                );
                assert_eq!(pane.offset(), (11, 23));
                assert!(pane.line_numbers());
                assert!(pane.is_err());
            } else {
                let Response::Pending(pending) = response else {
                    panic!("incomplete synchronous preview was published");
                };
                assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
                assert_empty(&pending.reader.cancel_any());
                release.send(Finish::Cancelled).unwrap();
            }
        }
    }
}

#[test]
fn from_workers_accepts_non_clone_send_only_workers_and_rejects_empty_input() {
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

    let error = PoolPreviewer::from_workers(std::iter::empty::<OwnedWorker>())
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let caller = thread::current().id();
    let created = Rc::new(Cell::new(0));
    let (started_tx, started) = unbounded();
    let (release, release_rx) = unbounded();
    let workers = (0..3).map(|_| {
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
    for rank in 0..3 {
        pending.push(submit(&mut pool, &picker, rank));
        let started = started.recv_timeout(WAIT).unwrap();
        assert_ne!(started.thread, caller);
        assert!(threads.insert(started.thread));
    }
    for _ in 0..3 {
        release.send(Finish::Publish).unwrap();
    }
    for (pending, expected) in pending.into_iter().zip(["first", "second", "third"]) {
        assert_ready(pending, expected);
    }
}

#[test]
fn default_workers_initialize_asynchronously_reserve_capacity_and_join_on_shutdown() {
    struct Control {
        created: Sender<thread::ThreadId>,
        initialize: Receiver<()>,
        started: Sender<Started>,
        release: Receiver<Finish>,
        dropped: Sender<thread::ThreadId>,
    }
    static CONTROL: OnceLock<Control> = OnceLock::new();

    struct DefaultWorker {
        thread: Rc<thread::ThreadId>,
        worker: TestWorker,
    }

    impl Default for DefaultWorker {
        fn default() -> Self {
            let control = CONTROL.get().unwrap();
            let thread = thread::current().id();
            control.created.send(thread).unwrap();
            control.initialize.recv_timeout(WAIT).unwrap();
            Self {
                thread: Rc::new(thread),
                worker: TestWorker {
                    started: control.started.clone(),
                    release: control.release.clone(),
                    calls: Cell::new(0),
                },
            }
        }
    }

    impl Worker<&'static str> for DefaultWorker {
        fn preview(
            &mut self,
            item: &&'static str,
            pane: &mut Pane,
            is_cancelled: impl Fn() -> bool,
        ) -> bool {
            assert_eq!(*self.thread, thread::current().id());
            self.worker.preview(item, pane, is_cancelled)
        }
    }

    impl Drop for DefaultWorker {
        fn drop(&mut self) {
            assert_eq!(*self.thread, thread::current().id());
            let _ = CONTROL.get().unwrap().dropped.send(*self.thread);
        }
    }

    let (created_tx, created) = unbounded();
    let (initialize, initialize_rx) = unbounded();
    let (started_tx, started) = unbounded();
    let (release, release_rx) = unbounded();
    let (dropped_tx, dropped) = unbounded();
    assert!(
        CONTROL
            .set(Control {
                created: created_tx,
                initialize: initialize_rx,
                started: started_tx,
                release: release_rx,
                dropped: dropped_tx,
            })
            .is_ok()
    );
    let mut pool =
        PoolPreviewer::with_default_workers::<DefaultWorker>(NonZeroUsize::new(2).unwrap())
            .unwrap();
    let threads: HashSet<_> = (0..2)
        .map(|_| created.recv_timeout(WAIT).unwrap())
        .collect();
    assert_eq!(threads.len(), 2);
    assert!(!threads.contains(&thread::current().id()));
    let picker = picker();
    initialize.send(()).unwrap();
    let background = prefetch(&mut pool, &picker, 0);
    let early = started.recv_timeout(QUIET).ok();
    let foreground = submit(&mut pool, &picker, 1);
    if early.is_some() {
        release.send(Finish::Publish).unwrap();
    }
    assert_eq!(started.recv_timeout(WAIT).unwrap().item, "second");
    release.send(Finish::Publish).unwrap();
    assert_ready(foreground, "second");
    initialize.send(()).unwrap();
    if early.is_none() {
        assert_eq!(started.recv_timeout(WAIT).unwrap().item, "first");
        release.send(Finish::Publish).unwrap();
    }
    assert_ready(background, "first");
    drop(pool);
    let destroyed: HashSet<_> = (0..2)
        .map(|_| dropped.recv_timeout(WAIT).unwrap())
        .collect();
    assert_eq!(destroyed, threads);
    assert!(early.is_none(), "initializing worker was treated as idle");

    let mut pool =
        PoolPreviewer::with_default_workers::<DefaultWorker>(NonZeroUsize::new(1).unwrap())
            .unwrap();
    let thread = created.recv_timeout(WAIT).unwrap();
    let pending = submit(&mut pool, &picker, 0);
    let shutdown = thread::spawn(move || drop(pool));
    assert!(completed(pending).is_none());
    assert!(!shutdown.is_finished());
    initialize.send(()).unwrap();
    shutdown.join().unwrap();
    assert_eq!(dropped.recv_timeout(WAIT).unwrap(), thread);
}

#[test]
fn workers_keep_state_but_receive_clean_panes_after_every_outcome() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    for (index, finish) in [
        Finish::Abandon,
        Finish::Cancelled,
        Finish::Publish,
        Finish::Publish,
    ]
    .into_iter()
    .enumerate()
    {
        let pending = submit(&mut pool, &picker, index);
        assert_eq!(started.recv_timeout(WAIT).unwrap().call, index + 1);
        if matches!(finish, Finish::Cancelled) {
            assert_empty(&pending.reader.cancel_any());
            release.send(finish).unwrap();
        } else {
            release.send(finish).unwrap();
            let result = completed(pending);
            if matches!(finish, Finish::Abandon) {
                assert!(result.is_none());
            } else {
                let pane = result.unwrap();
                assert_eq!(
                    pane.buffer().line(0).unwrap().as_str(),
                    ["first", "second", "third", "fourth"][index]
                );
                assert_eq!(pane.offset(), (3, 7));
                assert!(pane.line_numbers());
                assert!(pane.is_err());
            }
        }
    }
}

#[test]
fn shutdown_discards_queued_requests_and_waits_for_active_workers() {
    let picker = picker();
    let (mut pool, started, release) = pool(2);
    let active = [0, 1].map(|rank| {
        let pending = submit(&mut pool, &picker, rank);
        started.recv_timeout(WAIT).unwrap();
        pending
    });
    let queued = submit(&mut pool, &picker, 2);
    let shutdown = thread::spawn(move || drop(pool));
    assert!(completed(queued).is_none());
    assert!(!shutdown.is_finished());
    for _ in 0..2 {
        release.send(Finish::Publish).unwrap();
    }
    shutdown.join().unwrap();
    for (pending, expected) in active.into_iter().zip(["first", "second"]) {
        assert_ready(pending, expected);
    }
    assert!(started.try_recv().is_err());
}

#[test]
fn worker_panic_releases_requests_and_propagates_on_submission() {
    let picker = picker();
    let (mut pool, started, release) = pool(1);
    let panicking = submit(&mut pool, &picker, 0);
    started.recv_timeout(WAIT).unwrap();
    let queued = submit(&mut pool, &picker, 1);
    release.send(Finish::Panic).unwrap();
    assert!(completed(panicking).is_none());
    assert!(completed(queued).is_none());
    assert!(pool.is_closed());
    let panic = catch_unwind(AssertUnwindSafe(|| submit(&mut pool, &picker, 2)))
        .err()
        .expect("worker panic was not propagated");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"worker panic"));
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
    let pending = submit(&mut pool, &picker, 0);
    initialize.send(()).unwrap();
    assert!(completed(pending).is_none());
    assert!(pool.is_closed());
    let panic = catch_unwind(AssertUnwindSafe(|| submit(&mut pool, &picker, 1)))
        .err()
        .expect("initialization panic was not propagated");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"default worker panic"));
}
