use super::*;
use std::cell::Cell;
use std::sync::{
    Barrier,
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
};
use std::thread;

struct Counted {
    value: Cell<&'static str>,
    drops: Arc<AtomicUsize>,
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.drops.fetch_add(1, AtomicOrdering::Relaxed);
    }
}

fn counted(value: &'static str) -> (Counted, Arc<AtomicUsize>) {
    let drops = Arc::new(AtomicUsize::new(0));
    (
        Counted {
            value: Cell::new(value),
            drops: Arc::clone(&drops),
        },
        drops,
    )
}

#[test]
fn queued_only_cancellation_allows_active_publication_and_releases_values_once() {
    let (data, drops) = counted("original");
    let (reader, writer) = request(data);
    let active = writer.start().unwrap();
    let CancelQueued::Active(reader) = reader.cancel_queued() else {
        panic!("active request was cancelled");
    };
    let (mut completed, completed_drops) = counted("completed");
    assert!(active.publish(&mut completed));

    let Poll::Ready(data) = reader.poll() else {
        panic!("published request was not ready");
    };
    assert_eq!(data.value.get(), "completed");
    assert_eq!(completed.value.get(), "original");
    assert_eq!(drops.load(AtomicOrdering::Relaxed), 0);
    assert_eq!(completed_drops.load(AtomicOrdering::Relaxed), 0);
    drop(data);
    assert_eq!(completed_drops.load(AtomicOrdering::Relaxed), 1);
    drop(completed);
    assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
}

#[test]
fn cancellation_prevents_activation_or_publication_without_modifying_output() {
    let (reader, writer) = request(String::from("queued"));
    assert!(!writer.is_cancelled());
    let CancelQueued::Cancelled(data) = reader.cancel_queued() else {
        panic!("queued cancellation did not succeed");
    };
    assert_eq!(data, "queued");
    assert!(writer.is_cancelled());
    assert!(writer.start().is_none());
    let (reader, writer) = request(String::from("reusable"));
    let active = writer.start().unwrap();
    assert_eq!(reader.cancel_any(), "reusable");
    assert!(active.is_cancelled());

    let mut completed = String::from("done");
    assert!(!active.publish(&mut completed));
    assert_eq!(completed, "done");
}

#[test]
fn dropped_writers_relinquish_data() {
    for active in [false, true] {
        let (reader, writer) = request(String::from("original"));
        if active {
            drop(writer.start().unwrap());
        } else {
            drop(writer);
        }
        let Poll::Dropped(data) = reader.poll() else {
            panic!("dropped request was not reported as dropped");
        };
        assert_eq!(data, "original");
    }
}

#[test]
fn dropping_handles_in_either_order_releases_data_once() {
    for reader_first in [false, true] {
        for active in [false, true] {
            let (data, drops) = counted("data");
            let (reader, writer) = request(data);
            let exit = || {
                if active {
                    drop(writer.start().unwrap());
                } else {
                    drop(writer);
                }
            };
            if reader_first {
                drop(reader);
                exit();
            } else {
                exit();
                assert_eq!(drops.load(AtomicOrdering::Relaxed), 0);
                drop(reader);
            }
            assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        }
    }
}

#[test]
fn dropping_or_cancelling_during_activation_releases_data_once() {
    for (activate, cancel) in [(false, false), (true, false), (true, true)] {
        for _ in 0..1_000 {
            let (data, drops) = counted("data");
            let (reader, writer) = request(data);
            let barrier = &Barrier::new(2);

            thread::scope(|scope| {
                scope.spawn(move || {
                    barrier.wait();
                    if cancel {
                        drop(reader.cancel_any());
                    } else {
                        drop(reader);
                    }
                });
                scope.spawn(move || {
                    barrier.wait();
                    if activate {
                        drop(writer.start());
                    } else {
                        drop(writer);
                    }
                });
            });

            assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        }
    }
}

#[test]
fn cancel_and_publish_race_has_one_slot_owner() {
    for _ in 0..1_000 {
        let (data, drops) = counted("reusable");
        let (reader, writer) = request(data);
        let active = writer.start().unwrap();
        let (mut completed, completed_drops) = counted("completed");
        let barrier = &Barrier::new(2);

        thread::scope(|scope| {
            scope.spawn(move || {
                barrier.wait();
                let data = reader.cancel_any();
                assert!(matches!(data.value.get(), "reusable" | "completed"));
            });
            scope.spawn(move || {
                barrier.wait();
                if active.publish(&mut completed) {
                    assert_eq!(completed.value.get(), "reusable");
                } else {
                    assert_eq!(completed.value.get(), "completed");
                }
            });
        });
        assert_eq!(drops.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(completed_drops.load(AtomicOrdering::Relaxed), 1);
    }
}
