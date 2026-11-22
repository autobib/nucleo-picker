//! # The component abstraction
//!
//! Each component is only initialized for a single picker session. The component is given read-only
//! access to the underlying match engine and is responsible for component-specific event handling.
//!
//! A [`Component`] is a single part of the terminal interface. There are three core components:
//!
//! - the match list
//! - the status line
//! - the prompt
//!
//! as well as an additional component when enabled:
//!
//! - the previewer
//!
use crate::draw::Surface;
use crate::rect::{Dimensions, Position};

/// A single component in the frame.
pub(crate) trait Component {
    /// Resize the component, potentially taking into account the match data from the internal engine.
    fn resize(&mut self, _size: Dimensions) {}

    /// Draw this component in the given rectangle taking into account the match data from the
    /// internal engine. Make sure to take into account the rectangle drawing constraints.
    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error>;

    fn cursor(&self) -> Option<Position> {
        None
    }
}

impl<C: Component> Component for &mut C {
    fn resize(&mut self, size: Dimensions) {
        (**self).resize(size);
    }
    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        (**self).draw(surface)
    }
    fn cursor(&self) -> Option<Position> {
        (**self).cursor()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt;

    use crate::draw::{Style, StyledStr};

    use super::*;
    use crate::util::unicode::{Processor, UnicodeProcessor};

    pub(crate) struct TestSurface {
        size: Dimensions,
        column: u16,
        row: u16,
        pub rows: Vec<String>,
        pub spans: Vec<(String, Style)>,
        pub moves: Vec<(u16, u16)>,
        pub clear_calls: usize,
        pub touched: bool,
    }

    impl TestSurface {
        pub fn new(width: u16, height: u16) -> Self {
            Self {
                size: Dimensions { width, height },
                column: 0,
                row: 0,
                rows: vec![String::new(); usize::from(height)],
                spans: Vec::new(),
                moves: Vec::new(),
                clear_calls: 0,
                touched: false,
            }
        }
    }

    impl Surface for TestSurface {
        type Error = !;

        fn width(&self) -> u16 {
            self.size.width
        }

        fn height(&self) -> u16 {
            self.size.height
        }

        fn move_to(&mut self, column: u16, row: u16) -> Result<(), Self::Error> {
            assert!(column < self.size.width && row < self.size.height);
            self.touched = true;
            self.column = column;
            self.row = row;
            self.moves.push((column, row));
            Ok(())
        }

        fn move_to_column(&mut self, column: u16) -> Result<(), Self::Error> {
            self.move_to(column, self.row)
        }

        fn next_line(&mut self) -> Result<(), Self::Error> {
            assert!(!self.is_empty() && self.row < self.size.height);
            self.touched = true;
            self.column = 0;
            self.row += 1;
            Ok(())
        }

        fn styled_text<'a>(
            &mut self,
            text: &'a str,
            spans: impl Iterator<Item = StyledStr<'a>>,
        ) -> Result<(), Self::Error> {
            assert!(!self.is_empty() && self.row < self.size.height);
            let width = UnicodeProcessor::width(text);
            let end = usize::from(self.column) + width;
            assert!(
                end <= usize::from(self.size.width),
                "text overflows: {text:?}"
            );
            self.touched = true;
            self.rows[usize::from(self.row)].push_str(text);
            self.spans
                .extend(spans.map(|span| (span.text.to_owned(), span.style)));
            self.column = end as u16;
            Ok(())
        }

        fn ascii(&mut self, text: fmt::Arguments<'_>, style: Style) -> Result<(), Self::Error> {
            self.text(&text.to_string(), style)
        }

        fn spaces(&mut self, width: u16, style: Style) -> Result<(), Self::Error> {
            assert!(!self.is_empty());
            self.text(&" ".repeat(usize::from(width)), style)
        }

        fn clear_line(&mut self) -> Result<(), Self::Error> {
            self.clear_calls += 1;
            self.move_to_column(0)?;
            self.rows[usize::from(self.row)].clear();
            Ok(())
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.clear_calls += 1;
            if self.is_empty() {
                return Ok(());
            }
            self.move_to(0, 0)?;
            self.rows.iter_mut().for_each(String::clear);
            Ok(())
        }
    }
}
