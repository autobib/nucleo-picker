use std::io::{self, Write};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
    },
};

/// A generic terminal backend accepting raw bytes with ANSI escape sequences, with special
/// initialization and cleanup.
#[doc(hidden)]
pub trait Terminal: Write {
    /// Initialize the terminal for writing.
    ///
    /// For example, a proper backend might use this to enable raw mode, enter the alternate screen,
    /// and enable bracketed paste. This might also be a place to query the capabilities of the
    /// terminal emulator.
    fn init(&mut self) -> io::Result<()>;

    /// Clean up the terminal.
    ///
    /// This will be called when the terminal is no longer needed, or when the terminal is dropped
    /// because of an error or unwind.
    fn cleanup(&mut self) -> io::Result<()>;

    /// Report the size of the terminal.
    fn size(&mut self) -> io::Result<(u16, u16)>;
}

pub(crate) struct CrosstermTerminal<'a, W> {
    writer: &'a mut W,
}

impl<'a, W> CrosstermTerminal<'a, W> {
    pub(crate) fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }
}

impl<W: Write> Write for CrosstermTerminal<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> Terminal for CrosstermTerminal<'_, W> {
    fn init(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(self.writer, EnterAlternateScreen, EnableBracketedPaste)
    }

    fn cleanup(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(self.writer, DisableBracketedPaste, LeaveAlternateScreen)
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        size()
    }
}

/// A drop guard to increase the odds that the terminal is cleaned up on error or panic.
pub(crate) struct TerminalSession<'a, T: Terminal> {
    terminal: &'a mut T,
    cleanup: bool,
}

impl<'a, T: Terminal> TerminalSession<'a, T> {
    pub(crate) fn new(terminal: &'a mut T) -> Self {
        Self {
            terminal,
            cleanup: true,
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<()> {
        let result = self.terminal.cleanup();
        self.cleanup = false;
        result
    }
}

impl<T: Terminal> Write for TerminalSession<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.terminal.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.terminal.flush()
    }
}

impl<T: Terminal> Terminal for TerminalSession<'_, T> {
    fn init(&mut self) -> io::Result<()> {
        self.terminal.init()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.finish()
    }

    fn size(&mut self) -> io::Result<(u16, u16)> {
        self.terminal.size()
    }
}

impl<T: Terminal> Drop for TerminalSession<'_, T> {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = self.terminal.cleanup();
        }
    }
}
