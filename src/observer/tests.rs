use std::{sync::mpsc, thread};

use super::*;

#[test]
fn updates_coalesce_and_observer_clones_share_consumption() {
    let (notifier, observer) = occupied_channel(1);
    let other = observer.clone();

    notifier.push(2).unwrap();
    notifier.push(3).unwrap();
    assert_eq!(observer.try_peek(), Ok(3));
    assert_eq!(other.recv_timeout(Duration::ZERO), Ok(3));
    assert_eq!(observer.try_recv(), Err(TryRecvError::Empty));

    notifier.push(4).unwrap();
    other.clear();
    assert_eq!(observer.try_peek(), Err(TryRecvError::Empty));

    notifier.push(5).unwrap();
    drop(notifier);
    assert_eq!(observer.recv(), Ok(5));
    assert_eq!(other.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(
        observer.recv_timeout(Duration::ZERO),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn clearing_a_message_allows_its_destructor_to_observe_the_channel() {
    struct OnDrop {
        observer: Observer<Self>,
        dropped: mpsc::Sender<()>,
    }

    impl Drop for OnDrop {
        fn drop(&mut self) {
            assert!(matches!(self.observer.try_recv(), Err(TryRecvError::Empty)));
            self.dropped.send(()).unwrap();
        }
    }

    let (dropped, received) = mpsc::channel();
    let (notifier, observer) = channel();
    assert!(
        notifier
            .push(OnDrop {
                observer: observer.clone(),
                dropped,
            })
            .is_ok()
    );

    let clearing = thread::spawn(move || observer.clear());
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    clearing.join().unwrap();
}
