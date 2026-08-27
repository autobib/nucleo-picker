use std::io::{self, Write};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
    },
};

#[cfg(all(test, feature = "unstable-backend"))]
mod size_test;

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
    ///
    /// This method is only called immediately writes are required, and may not be called on each
    /// frame.
    fn size(&mut self) -> io::Result<(u16, u16)>;

    /// Begin rendering a frame.
    ///
    /// This method is called immediately before writes occur at the start of a frame. The default
    /// implementation does nothing.
    fn begin_render(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// End rendering a frame.
    ///
    /// This method is called immediately after a frame render ends. The default
    /// implementation calls `self.flush()`.
    fn end_render(&mut self) -> io::Result<()> {
        self.flush()
    }

    /// A callback executed at the end of each frame.
    ///
    /// This method is called on every frame, whether or not it was rendered. The boolean indicates
    /// if there were logical changes to the picker state since the previous frame.
    ///
    /// If `changed` is `false`, writes definitely did not occur. If `changed` is true, it is
    /// possible that writes still may not have occurred. Currently, this is only the case
    /// if the terminal has width 0, but future versions may perform more sophisticated checks
    /// to see if there were no changes to the actual screen content, and suppress writes
    /// in that case as well.
    ///
    /// For operations which should only be performed if writes actually occurr, override
    /// [`begin_render`](Self::begin_render) or [`end_render`](Self::end_render).
    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        let _ = changed;
        Ok(())
    }
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

    fn end_frame(&mut self, changed: bool) -> io::Result<()> {
        self.terminal.end_frame(changed)
    }
}

impl<T: Terminal> Drop for TerminalSession<'_, T> {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = self.terminal.cleanup();
        }
    }
}
