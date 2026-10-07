use super::*;
use std::sync::mpsc;

struct TestEvents<F>(F);

impl<F: FnMut(Duration) -> Result<Event, RecvError>> EventSource for TestEvents<F> {
    type AbortErr = Infallible;

    fn recv_timeout(&mut self, duration: Duration) -> Result<Event, RecvError> {
        (self.0)(duration)
    }
}

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

#[test]
fn default_try_recv_preserves_io_errors() {
    let mut source = TestEvents(|duration: Duration| {
        assert!(duration.is_zero());
        Err(RecvError::IO(io::ErrorKind::BrokenPipe.into()))
    });

    assert!(
        matches!(source.try_recv(), Err(RecvError::IO(err)) if err.kind() == io::ErrorKind::BrokenPipe)
    );
}
