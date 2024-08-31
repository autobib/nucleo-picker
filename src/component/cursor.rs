//! # A windowed cursor
//! This module implements a windowed cursor component.
use std::{
    cmp::min,
    fmt::{Display, Formatter},
};

/// # A view of an underlying buffer.
#[derive(Debug, Clone, Copy)]
pub struct View<'a, T> {
    contents: &'a [T],
    _index: usize,
}

impl Display for View<'_, char> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for ch in self.contents {
            ch.fmt(f)?
        }
        Ok(())
    }
}

impl<'a, T> View<'a, T> {
    /// The `0`-indexed position of the cursor within the view, which is guaranteed to be a valid
    /// index for the slice returned by [`View::as_slice`].
    pub fn index(&self) -> usize {
        self._index
    }

    /// Return the contents of the view.
    pub fn as_slice(&self) -> &'a [T] {
        self.contents
    }
}

/// # Represent a cursor and window within an underlying buffer.
/// Represent the position of a cursor within an underlying buffer, along with a corresponding
/// window which contains the cursor. To change the cursor position, use the [`Cursor::set_index`]
/// function. To use the cursor to obtain a slice of an underlying buffer, use the [`Cursor::view`]
/// or [`Cursor::view_padded`] function.
///
///
/// ## Representing the windowed cursor
/// The internal state is represented by three parameters:
/// - The `index`, which is the position within the string.
/// - The `width`, which is the width of the internal window
/// - The `offset`, which is the position of the leftmost endpoint of the view window.
///
/// Visually, given an underlying buffer
/// ```txt
/// _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _
/// ```
/// a windowed cursor is represented, for instance,
/// ```txt
/// _ _ _ _[* _ _ _ _ _]_ _ _ _ _ _ _
/// ```
/// which has `index = 5`, `width = 6`, and `offset = 4`.
/// The main invariant is that `offset <= index < offset + width`.
///
///
/// ## Moving the cursor
/// When the cursor is moved using the [`Cursor::set_index`] function, the offset also changes so
/// that the cursor is contained within the window, if necessary. The offset is always changed by
/// the minimal amount to ensure that the cursor is contained in the window.
///
/// For instance,
/// ```txt
/// _ _ _ _[_ _ _ _ _ _]_ * _ _ _ _ _
/// ```
/// becomes
/// ```txt
/// _ _ _ _ _ _[_ _ _ _ _ *]_ _ _ _ _
/// ```
/// and
/// ```txt
/// _ * _ _ _ _ _[_ _ _ _ _ _]_ _ _ _
/// ```
/// becomes
/// ```txt
/// _[* _ _ _ _ _]_ _ _ _ _ _ _ _ _ _
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    /// The position within the string.
    index: usize,
    /// The current offset of the view window.
    offset: usize,
    /// The width of the view window.
    width: usize,
}

impl Cursor {
    /// Create a new cursor with given width.
    pub fn new(width: usize) -> Self {
        Self {
            index: 0,
            offset: 0,
            width,
        }
    }

    /// The absolute index of the cursor.
    pub fn idx(&self) -> usize {
        self.index
    }

    /// Return the padded view given the current cursor position with padding size on the left
    /// and the right.
    ///
    /// The padding will translate to the right if the view would contain negative indices.
    /// For example, a left padding of 2 and a right padding of 2 when the index is 1 would result
    /// in 3 extra characters on the right side of the view:
    /// ```txt
    /// {_[* _ _ _ _ _]_ _ _}_ _ _ _ _ _ _
    /// ```
    pub fn view_padded<'a, T>(&self, left: usize, right: usize, buffer: &'a [T]) -> View<'a, T> {
        let view_left = self.offset.saturating_sub(left);
        let view_right = min(view_left + left + self.width + right, buffer.len());
        View {
            contents: &buffer[view_left..view_right],
            _index: self.index - view_left,
        }
    }

    /// Return the current view given the current cursor state. For the padded version, see
    /// [`Self::view_padded`].
    #[inline]
    pub fn view<'a, T>(&self, target: &'a [T]) -> View<'a, T> {
        self.view_padded(0, 0, target)
    }

    /// Set the index to the given value.
    pub fn set_index(&mut self, index: usize) {
        self.index = index;
        self.reset()
    }

    /// Set the width to the given value.
    pub fn set_width(&mut self, width: usize) {
        self.width = width;
        self.reset()
    }

    /// Reset the internal window.
    fn reset(&mut self) {
        if self.index < self.offset {
            self.offset = self.index
        } else if self.index >= self.offset + self.width {
            self.offset = self.index - self.width
        }
    }
}
