use std::{io, sync::mpsc, thread, time::Duration};

use crate::{
    Picker, PickerOptions, Terminal,
    component::NoPreview,
    event::{Event, PromptEvent},
    render::StrRenderer,
};

struct TestTerminal;

impl io::Write for TestTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for TestTerminal {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        Ok((20, 8))
    }
}

fn pick(
    picker: &mut Picker<&'static str, StrRenderer>,
    events: impl IntoIterator<Item = Event>,
) -> Option<&'static str> {
    let (sender, receiver) = mpsc::channel();
    for event in events {
        sender.send(event).unwrap();
    }
    drop(sender);
    picker
        .pick_impl::<_, _, (), _>(receiver, &mut TestTerminal, NoPreview::new())
        .unwrap()
        .copied()
}

#[test]
fn initial_query_matches_the_normalized_prompt() {
    let mut picker = PickerOptions::new().query("a\u{7}b").picker(StrRenderer);
    picker.extend(["ab", "cd"]);
    while picker.engine.update(5).matching {}

    assert_eq!(picker.query(), "ab");
    assert_eq!(picker.engine.snapshot().matched_item_count(), 1);
    assert_eq!(pick(&mut picker, [Event::Select]), Some("ab"));

    pick(
        &mut picker,
        [
            Event::Prompt(PromptEvent::Reset("ab".to_owned())),
            Event::Quit,
        ],
    );
    while picker.engine.update(5).matching {}

    assert_eq!(picker.query(), "ab");
    assert_eq!(picker.engine.snapshot().matched_item_count(), 1);
    assert_eq!(pick(&mut picker, [Event::Select]), Some("ab"));
}

#[test]
fn quit_preserves_the_buffered_prompt_event() {
    let mut picker = Picker::new(StrRenderer);

    assert_eq!(
        pick(
            &mut picker,
            [Event::Prompt(PromptEvent::Insert('x')), Event::Quit],
        ),
        None,
    );
    assert_eq!(picker.query(), "x");
}

#[test]
fn select_preserves_the_buffered_prompt_event_and_current_selection() {
    let mut picker = Picker::new(StrRenderer);
    picker.extend(["alpha"]);
    while picker.engine.update(5).matching {}

    assert_eq!(
        pick(
            &mut picker,
            [Event::Prompt(PromptEvent::Insert('x')), Event::Select],
        ),
        Some("alpha"),
    );
    assert_eq!(picker.query(), "x");
}

#[test]
fn reused_picker_matches_the_query_saved_on_exit() {
    let mut picker = Picker::new(StrRenderer);
    picker.extend(["alpha", "beta"]);
    while picker.engine.update(5).matching {}

    pick(
        &mut picker,
        [
            Event::Prompt(PromptEvent::Reset("beta".to_owned())),
            Event::Quit,
        ],
    );
    let status = picker.status_observer();
    let (sender, receiver) = mpsc::channel::<Event>();
    let input = thread::spawn(move || {
        for id in 0.. {
            sender.send(Event::Status { id }).unwrap();
            if !status
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .matching
            {
                break;
            }
        }
        sender.send(Event::Select).unwrap();
    });
    let selected = picker
        .pick_impl::<_, _, (), _>(receiver, &mut TestTerminal, NoPreview::new())
        .unwrap()
        .copied();
    input.join().unwrap();

    assert_eq!(selected, Some("beta"));
    assert_eq!(picker.query(), "beta");
}
