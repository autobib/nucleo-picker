use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use crate::{Picker, event::Event, render::StrRenderer};

use super::Terminal;

struct PanickingTerminal {
    cleaned: Arc<AtomicBool>,
}

impl Write for PanickingTerminal {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("terminal write panic");
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for PanickingTerminal {
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.cleaned.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        Ok((20, 8))
    }
}

#[test]
fn panic_cleans_up_the_supplied_terminal() {
    let cleaned = Arc::new(AtomicBool::new(false));
    let mut terminal = PanickingTerminal {
        cleaned: Arc::clone(&cleaned),
    };
    let mut picker: Picker<String, _> = Picker::new(StrRenderer);
    let (_sender, events) = mpsc::channel::<Event>();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = picker.pick_with_terminal_io(events, &mut terminal);
    }));

    assert!(panic.is_err());
    assert!(cleaned.load(Ordering::SeqCst));
}
