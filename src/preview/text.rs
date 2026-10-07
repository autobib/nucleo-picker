#[cfg(test)]
mod tests;

use std::fmt;

use super::PreviewBuffer;

/// A [`fmt::Write`] adapter to simplify repeated calls to [`PreviewBuffer::push_text`].
///
/// Note that this writer only supports unstyled writes. For styled writes, use the corresponding
/// methods directly or see [`AnsiWriter`](super::AnsiWriter).
#[derive(Debug)]
pub struct TextWriter<'a> {
    buffer: &'a mut PreviewBuffer,
    pending_cr: bool,
}

impl<'a> TextWriter<'a> {
    /// Initialize a new text writer around an existing buffer.
    pub fn new(buffer: &'a mut PreviewBuffer) -> Self {
        Self {
            buffer,
            pending_cr: false,
        }
    }
}

impl PreviewBuffer {
    /// Returns a [`fmt::Write`] adapter which defers writes to [`push_text`](Self::push_text).
    ///
    /// See the [`push_text`](Self::push_text) documentation for the behaviour of this method. Note
    /// that this only supports writing unstyled text. Ensure to use a single writer for the entire
    /// input stream: dropping and reopening this writer can result in slightly different buffer
    /// content.
    pub fn text_writer(&mut self) -> TextWriter<'_> {
        TextWriter::new(self)
    }
}

impl fmt::Write for TextWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.is_empty() {
            return Ok(());
        }

        if self.pending_cr && !text.starts_with('\n') {
            self.buffer.push_text("\r");
        }
        let (text, pending_cr) = match text.strip_suffix('\r') {
            Some(text) => (text, true),
            None => (text, false),
        };
        self.buffer.push_text(text);
        self.pending_cr = pending_cr;
        Ok(())
    }
}

impl Drop for TextWriter<'_> {
    fn drop(&mut self) {
        if self.pending_cr {
            self.buffer.push_text("\r");
        }
    }
}
