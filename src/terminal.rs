use std::{
    io::{self, Write},
    mem::DropGuard,
};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
    },
};

use crate::rect::Dimensions;

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
    fn dimensions(&mut self) -> io::Result<Dimensions>;

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
    #[inline]
    fn end_render(&mut self) -> io::Result<()> {
        self.flush()
    }

    /// A callback executed at the end of each frame.
    ///
    /// This method is called on every frame (except initialization), whether or not it was rendered. The boolean indicates
    /// whether there were session invalidations or host presentation changes since the previous
    /// frame, including layout changes and explicit redraw requests.
    ///
    /// If `changed` is `false`, writes definitely did not occur. If `changed` is true, it is
    /// possible that writes still may not have occurred. Currently, this is only the case
    /// if the terminal has width or height 0, but future versions may perform more sophisticated checks
    /// to see if there were no changes to the actual screen content, and suppress writes
    /// in that case as well.
    ///
    /// For operations which should only be performed if writes actually occur, override
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
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    #[inline]
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

    #[inline]
    fn dimensions(&mut self) -> io::Result<Dimensions> {
        let (width, height) = size()?;
        Ok(Dimensions { width, height })
    }
}

/// A drop guard to increase the odds that the terminal is cleaned up on error or panic.
pub(crate) fn session<'a, T: Terminal>(
    terminal: &'a mut T,
) -> DropGuard<&'a mut T, impl FnOnce(&'a mut T)> {
    DropGuard::new(terminal, |terminal: &mut T| {
        let _ = terminal.cleanup();
    })
}
