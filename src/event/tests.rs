use super::*;
use std::sync::mpsc;

#[test]
fn channel_try_recv_preserves_events_before_disconnection() {
    let (sender, mut receiver) = mpsc::channel::<Event>();
    assert!(matches!(
        EventSource::try_recv(&mut receiver),
        Err(RecvError::Timeout)
    ));

    sender.send(Event::Status { id: 3 }).unwrap();
    sender.send(Event::Quit).unwrap();
    drop(sender);

    assert!(matches!(
        EventSource::try_recv(&mut receiver),
        Ok(Event::Status { id: 3 })
    ));
    assert!(matches!(
        EventSource::try_recv(&mut receiver),
        Ok(Event::Quit)
    ));
    assert!(matches!(
        EventSource::try_recv(&mut receiver),
        Err(RecvError::Disconnected)
    ));
}
