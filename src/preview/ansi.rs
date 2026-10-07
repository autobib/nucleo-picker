#[cfg(test)]
mod tests;

use std::io::{self, Write};

use anstyle_parse::{Params, Parser, Perform};
use crossterm::style::{Attribute, Color, ContentStyle};

use super::PreviewBuffer;

/// Write an ANSI byte-stream directly into a preview buffer.
///
/// The conventional way to construct this type is through [`PreviewBuffer::ansi_writer`]. This type
/// implements [`io::Write`] and can be used to write an ANSI byte-stream directly into the
/// underlying buffer.
#[cfg_attr(docsrs, doc(cfg(feature = "preview-ansi")))]
#[derive(Debug)]
pub struct AnsiWriter<'a> {
    parser: Parser,
    sink: Sink<'a>,
}

impl<'a> AnsiWriter<'a> {
    /// Initialize a new writer around the provided buffer.
    ///
    /// Also see [`PreviewBuffer::ansi_writer`].
    pub fn new(buffer: &'a mut PreviewBuffer) -> Self {
        Self {
            parser: Parser::default(),
            sink: Sink {
                buffer,
                style: ContentStyle::default(),
            },
        }
    }
}

impl PreviewBuffer {
    /// Write a stream of bytes with ANSI-encoded styling directly into this buffer.
    ///
    /// It is assumed that the input stream is UTF-8 encoded by convenention. Invalid UTF-8 is
    /// handled using lossy conversion.
    ///
    /// Note that writes require holding some parser state, so make sure the entire byte stream is
    /// written before closing this type. Closing and re-opening the writer on the same stream is
    /// *not equivalent* to using a single writer continuously.
    ///
    /// ## Example
    ///
    /// ```
    /// use std::io::Write;
    /// use nucleo_picker::preview::PreviewBuffer;
    ///
    /// let mut buffer = PreviewBuffer::new();
    /// buffer.ansi_writer().write_all(b"\x1b[1;31merror\x1b[0m: missing file\n")?;
    /// assert_eq!(buffer.line(0).unwrap().as_str(), "error: missing file");
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "preview-ansi")))]
    pub fn ansi_writer(&mut self) -> AnsiWriter<'_> {
        AnsiWriter {
            parser: Parser::default(),
            sink: Sink {
                buffer: self,
                style: ContentStyle::default(),
            },
        }
    }
}

impl Write for AnsiWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for &byte in bytes {
            self.parser.advance(&mut self.sink, byte);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct Sink<'a> {
    buffer: &'a mut PreviewBuffer,
    style: ContentStyle,
}

impl Perform for Sink<'_> {
    fn print(&mut self, ch: char) {
        if !ch.is_control() {
            self.buffer
                .push_styled_str(ch.encode_utf8(&mut [0; 4]), self.style);
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.buffer.newline(),
            b'\t' => self.print('␉'),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: u8) {
        if ignore
            || action != b'm'
            || !intermediates.is_empty()
            || params.iter().any(|param| param.len() != 1)
        {
            return;
        }

        use Attribute::*;

        let mut params = params.iter().map(|param| param[0]);
        while let Some(code) = params.next() {
            match code {
                0 => self.style = ContentStyle::default(),
                1 => self.style.attributes.set(Bold),
                2 => self.style.attributes.set(Dim),
                3 => self.style.attributes.set(Italic),
                4 => {
                    self.style.attributes.unset(DoubleUnderlined);
                    self.style.attributes.set(Underlined);
                }
                5 => {
                    self.style.attributes.unset(RapidBlink);
                    self.style.attributes.set(SlowBlink);
                }
                6 => {
                    self.style.attributes.unset(SlowBlink);
                    self.style.attributes.set(RapidBlink);
                }
                7 => self.style.attributes.set(Reverse),
                8 => self.style.attributes.set(Hidden),
                9 => self.style.attributes.set(CrossedOut),
                21 => {
                    self.style.attributes.unset(Underlined);
                    self.style.attributes.set(DoubleUnderlined);
                }
                22 => {
                    self.style.attributes.unset(Bold);
                    self.style.attributes.unset(Dim);
                }
                23 => self.style.attributes.unset(Italic),
                24 => {
                    self.style.attributes.unset(Underlined);
                    self.style.attributes.unset(DoubleUnderlined);
                }
                25 => {
                    self.style.attributes.unset(SlowBlink);
                    self.style.attributes.unset(RapidBlink);
                }
                27 => self.style.attributes.unset(Reverse),
                28 => self.style.attributes.unset(Hidden),
                29 => self.style.attributes.unset(CrossedOut),
                30..=37 => self.style.foreground_color = Some(Color::AnsiValue((code - 30) as u8)),
                40..=47 => self.style.background_color = Some(Color::AnsiValue((code - 40) as u8)),
                90..=97 => {
                    self.style.foreground_color = Some(Color::AnsiValue((code - 90 + 8) as u8));
                }
                100..=107 => {
                    self.style.background_color = Some(Color::AnsiValue((code - 100 + 8) as u8));
                }
                38 | 48 | 58 => {
                    let Some(color) = extended_color(&mut params) else {
                        break;
                    };
                    match code {
                        38 => self.style.foreground_color = Some(color),
                        48 => self.style.background_color = Some(color),
                        58 => self.style.underline_color = Some(color),
                        _ => unreachable!(),
                    }
                }
                39 => self.style.foreground_color = None,
                49 => self.style.background_color = None,
                59 => self.style.underline_color = None,
                _ => {}
            }
        }
    }
}

fn extended_color(params: &mut impl Iterator<Item = u16>) -> Option<Color> {
    match params.next()? {
        5 => Some(Color::AnsiValue(u8::try_from(params.next()?).ok()?)),
        2 => Some(Color::Rgb {
            r: u8::try_from(params.next()?).ok()?,
            g: u8::try_from(params.next()?).ok()?,
            b: u8::try_from(params.next()?).ok()?,
        }),
        _ => None,
    }
}
