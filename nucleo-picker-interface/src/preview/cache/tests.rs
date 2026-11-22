use super::*;
use crate::preview::request::{QueuedRequest, Request};

fn polled_queued_request() -> (Pending, QueuedRequest<&'static str>) {
    let mut picker =
        nucleo_picker_core::session::Model::new(nucleo_picker_core::render::StrRenderer);
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    let matches = picker.matches();
    let (pending, queued) = Request {
        buffer: Buffer::new(),
        epoch: 1,
        matches,
        id: matches.get(0).unwrap().id(),
    }
    .defer();
    let RequestState::Pending(pending) = RequestState::Pending(pending).poll() else {
        panic!("expected queued preview");
    };
    (pending, queued)
}

#[test]
fn promotion_preserves_work_that_starts_after_polling() {
    let (pending, queued) = polled_queued_request();
    let active = queued.start().unwrap();
    let RequestState::Active(pending) = pending.reprioritize() else {
        panic!("expected active preview");
    };
    assert!(!active.is_cancelled());
    let buffer = Buffer::new();
    assert!(active.publish(&mut buffer.into_pane()).is_ok());
    assert!(matches!(
        RequestState::Active(pending).poll(),
        RequestState::Ready(_)
    ));
}

#[test]
fn promotion_collects_work_published_after_polling() {
    let (pending, queued) = polled_queued_request();
    let mut buffer = Buffer::new();
    buffer.push_str("completed");
    assert!(
        queued
            .start()
            .unwrap()
            .publish(&mut buffer.into_pane())
            .is_ok()
    );
    let RequestState::Ready(pane) = pending.reprioritize() else {
        panic!("expected ready preview");
    };
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "completed");
}

#[test]
fn promotion_reclaims_work_dropped_after_polling() {
    let (pending, queued) = polled_queued_request();
    drop(queued);
    assert!(matches!(pending.reprioritize(), RequestState::Retry(_)));
}
