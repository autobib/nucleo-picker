#[cfg(test)]
mod tests;

use std::ops::Range;

use crossterm::style::{ContentStyle, StyledContent};
use memchr::memchr;

/// A buffer holding the contents of a single preview pane.
///
/// This is an append-only buffer. The buffer consists of a vector of completed lines as well as a
/// final active line. An empty buffer consists only of an empty active line.
/// There are two main input modes:
///
/// - Segment-based input: [`push_str`](Self::push_str) and
///   [`push_styled_str`](Self::push_styled_str). These methods append text to the current active
///   line, and the caller promises that the pushed text does not contain any line breaks or other
///   control characters. Use [`newline`](Self::newline) to create line breaks. These methods are
///   most useful when you want fine-grained control over how text is added to the preview buffer.
/// - Text-based input: [`push_text`](Self::push_text) and
///   [`push_styled_text`](Self::push_styled_text). These convenience methods accept strings which
///   may contain line breaks and control characters. Line breaks are processed to break input
///   into lines, and control characters are either substituted or discarded. These methods
///   are most useful for rendering untrusted text directly into the preview buffer.
///
/// # Example
///
/// Render a BibTeX entry with styled field names and values.
///
/// ```
/// use std::collections::BTreeMap;
/// use crossterm::style::{ContentStyle, Stylize};
/// use nucleo_picker::preview::PreviewBuffer;
///
/// // In this example, `kind` and `citation_key` are strings which we know do not contain control
/// // characters, but the field values may contain newlines or control characters.
/// fn render_entry(
///     buffer: &mut PreviewBuffer,
///     kind: &str,
///     citation_key: &str,
///     fields: &BTreeMap<&str, &str>,
/// ) {
///     let keyword = ContentStyle::new().magenta().bold();
///     let identifier = ContentStyle::new().cyan();
///     let field_name = ContentStyle::new().blue();
///     let value_style = ContentStyle::new().green();
///
///     buffer.clear();
///
///     buffer.push_str("@");
///     buffer.push_styled_str(kind, keyword);
///     buffer.push_str("{");
///     buffer.push_styled_str(citation_key, identifier);
///     buffer.push_str(",");
///     buffer.newline();
///
///     for (&name, &value) in fields {
///         buffer.push_str("  ");
///         buffer.push_styled_str(name, field_name);
///         buffer.push_str(" = {");
///         buffer.push_styled_text(value, value_style);
///         buffer.push_str("},");
///         buffer.newline();
///     }
///
///     buffer.push_str("}");
/// }
///
/// let fields = BTreeMap::from([
///     ("author", "John Doe"),
///     ("title", "A title which is very long and\n           contains an internal newline"),
///     ("year", "2025"),
/// ]);
/// let mut buffer = PreviewBuffer::new();
/// render_entry(&mut buffer, "article", "key", &fields);
/// // @article{key,
/// //   author = {John Doe},
/// //   title = {A title which is very long and
/// //            contains an internal newline},
/// //   year = {2025},
/// // }
/// # assert_eq!(
/// #     buffer.lines().map(|line| line.as_str()).collect::<Vec<_>>(),
/// #     [
/// #         "@article{key,",
/// #         "  author = {John Doe},",
/// #         "  title = {A title which is very long and",
/// #         "           contains an internal newline},",
/// #         "  year = {2025},",
/// #         "}",
/// #     ]
/// # );
/// # assert_eq!(
/// #     buffer.line(3).unwrap().spans().next().unwrap(),
/// #     ContentStyle::new().green().apply("           contains an internal newline")
/// # );
/// ```
#[derive(Debug)]
pub struct PreviewBuffer {
    text: String,
    lines: Vec<LineIndex>,
    styles: Vec<StyleSpan>,
    is_err: bool,
}

#[derive(Debug)]
struct LineIndex {
    text: Range<usize>,
    styles: Range<usize>,
}

#[derive(Debug)]
struct StyleSpan {
    bytes: Range<usize>,
    style: ContentStyle,
}

/// A view of a single styled line of a preview buffer.
///
/// A line consists of the underlying text (excluding line breaks) as well
/// as the style spans.
#[derive(Clone, Copy, Debug)]
pub struct PreviewLine<'a> {
    text: &'a str,
    styles: &'a [StyleSpan],
}

impl Default for PreviewBuffer {
    fn default() -> Self {
        Self {
            text: String::new(),
            lines: vec![LineIndex {
                text: 0..0,
                styles: 0..0,
            }],
            styles: Vec::new(),
            is_err: false,
        }
    }
}

impl PreviewBuffer {
    /// Initialize a new buffer containing a single empty line.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear this buffer, retaining the underlying allocations.
    pub fn clear(&mut self) {
        self.text.clear();
        self.lines.clear();
        self.styles.clear();
        self.push_empty_line();
        self.is_err = false;
    }

    /// Set the failure status of the buffer.
    pub fn set_err(&mut self, is_err: bool) {
        self.is_err = is_err;
    }

    /// Get the failure status of the buffer.
    pub fn is_err(&self) -> bool {
        self.is_err
    }

    /// Append an unstyled segment to the current line in the buffer.
    ///
    /// The text must not contain control characters. This is a convenience call to
    /// [`push_styled_str`](Self::push_styled_str) with [`ContentStyle::default()`].
    /// See that method for more details.
    ///
    /// # Panics
    ///
    /// Panics without modifying the buffer if `text` contains a newline.
    pub fn push_str(&mut self, text: &str) {
        self.push_styled_str(text, ContentStyle::default());
    }

    /// Append an unstyled segment to the current line in the buffer in the given style.
    ///
    /// The text must not contain control characters, or the terminal window will be corrupted.
    ///
    /// The provided text is appended to the current active line. To start a new line, use
    /// [`newline`](Self::newline).
    ///
    /// # Panics
    ///
    /// Panics without modifying the buffer if `text` contains a newline.
    pub fn push_styled_str(&mut self, text: &str, style: ContentStyle) {
        assert!(
            memchr(b'\n', text.as_bytes()).is_none(),
            "preview segments must not contain LF; use newline()"
        );
        if text.is_empty() {
            return;
        }

        self.text.push_str(text);
        let line = self.lines.last_mut().unwrap();
        let start = line.text.len();
        line.text.end = self.text.len();
        let end = line.text.len();
        if style == ContentStyle::default() {
            return;
        }

        if !line.styles.is_empty()
            && let Some(last) = self.styles.last_mut()
            && last.bytes.end == start
            && last.style == style
        {
            last.bytes.end = end;
        } else {
            self.styles.push(StyleSpan {
                bytes: start..end,
                style,
            });
            line.styles.end += 1;
        }
    }

    /// Append unstyled text and process newlines and control characters automatically.
    ///
    /// This is a convenience method to call [`push_styled_text`](Self::push_styled_text) with
    /// the default style. See that method for more details.
    pub fn push_text(&mut self, text: &str) {
        self.push_styled_text(text, ContentStyle::default());
    }

    /// Append styled text and process newlines and control characters automatically.
    ///
    /// Internally, this method calls [`push_styled_str`](Self::push_styled_str) and
    /// [`newline`](Self::newline) as follows:
    ///
    /// - Empty input does nothing.
    /// - Line endings are converted into calls to [`newline`](Self::newline).
    /// - Segments not containing control characters are written using [`push_styled_str`](Self::push_styled_str)
    /// - C0 control characters and DEL become Unicode control placeholders. C1 controls are
    ///   discarded.
    pub fn push_styled_text(&mut self, text: &str, style: ContentStyle) {
        for chunk in text.split_inclusive('\n') {
            let (line, newline) = match chunk.strip_suffix('\n') {
                Some(line) => (line.strip_suffix('\r').unwrap_or(line), true),
                None => (chunk, false),
            };
            let mut start = 0;
            for (offset, ch) in line.char_indices().filter(|(_, ch)| ch.is_control()) {
                self.push_styled_str(&line[start..offset], style);
                let picture = match ch {
                    '\x00'..='\x1f' => char::from_u32(0x2400 + u32::from(ch)),
                    '\x7f' => Some('\u{2421}'),
                    _ => None,
                };
                if let Some(picture) = picture {
                    self.push_styled_str(picture.encode_utf8(&mut [0; 4]), style);
                }
                start = offset + ch.len_utf8();
            }
            self.push_styled_str(&line[start..], style);
            if newline {
                self.newline();
            }
        }
    }

    /// Complete the current line and start the next empty line.
    pub fn newline(&mut self) {
        self.push_empty_line();
    }

    /// Append an unstyled segment to the current line, then start a new line.
    ///
    /// This calls [`push_styled_line`](Self::push_styled_line) with the default style.
    ///
    /// # Panics
    ///
    /// Panics before modifying the buffer if `text` contains a newline.
    pub fn push_line(&mut self, text: &str) {
        self.push_styled_line(text, ContentStyle::default());
    }

    /// Append a segment in the given style to the current line, then start a new line.
    ///
    /// This is a convenience method to call [`push_styled_str`](Self::push_styled_str), followed by
    /// [`newline`](Self::newline). Note that a line break will still be produced even if the input
    /// is empty.
    ///
    /// # Panics
    ///
    /// Panics before modifying the buffer if `text` contains LF.
    pub fn push_styled_line(&mut self, text: &str, style: ContentStyle) {
        self.push_styled_str(text, style);
        self.newline();
    }

    fn push_empty_line(&mut self) {
        let text_start = self.text.len();
        let style_start = self.styles.len();
        self.lines.push(LineIndex {
            text: text_start..text_start,
            styles: style_start..style_start,
        });
    }

    /// Get the `n`th line in the buffer as a string slice, excluding the newline and carriage
    /// return (if any).
    pub fn line(&self, index: usize) -> Option<PreviewLine<'_>> {
        self.lines.get(index).map(|line| self.line_view(line))
    }

    /// Iterate over the all of the lines in this buffer.
    pub fn lines(&self) -> impl DoubleEndedIterator<Item = PreviewLine<'_>> + ExactSizeIterator {
        self.lines.iter().map(|line| self.line_view(line))
    }

    fn line_view(&self, line: &LineIndex) -> PreviewLine<'_> {
        PreviewLine {
            text: &self.text[line.text.clone()],
            styles: &self.styles[line.styles.clone()],
        }
    }
}

impl<'a> PreviewLine<'a> {
    /// Returns the raw text of the current line.
    pub fn as_str(&self) -> &'a str {
        self.text
    }

    /// Consume this line and return an iterator over styled content.
    pub fn spans(self) -> impl Iterator<Item = StyledContent<&'a str>> {
        let Self { text, mut styles } = self;
        let mut offset = 0;
        std::iter::from_fn(move || {
            if offset == text.len() {
                return None;
            }

            let (end, style) = match styles.first() {
                Some(span) if offset < span.bytes.start => {
                    (span.bytes.start, ContentStyle::default())
                }
                Some(span) => {
                    styles = &styles[1..];
                    (span.bytes.end, span.style)
                }
                None => (text.len(), ContentStyle::default()),
            };
            let content = &text[offset..end];
            offset = end;
            Some(style.apply(content))
        })
    }
}
