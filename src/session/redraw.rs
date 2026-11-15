use std::ops::{BitOr, BitOrAssign};

/// A redraw mask for the picker interface.
///
/// - `NONE` is the default (no redraw).
/// - `QUEUE` requests redraw for queue markers and the status line..
/// - `PREVIEW` requests redraw for the preview pane.
///
/// Use `|` and `|=` to combine masks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Redraw {
    pub(crate) prompt: bool,
    pub(crate) list: bool,
    pub(crate) status: bool,
    pub(crate) preview: bool,
}

impl Redraw {
    /// The empty redraw mask, equivalent to the [`Default`] implementation.
    pub const NONE: Self = Self {
        prompt: false,
        list: false,
        status: false,
        preview: false,
    };

    /// Request redraw of queue markers and status line.
    pub const QUEUE: Self = Self {
        list: true,
        status: true,
        ..Self::NONE
    };

    /// Request redraw of the preview pane.
    pub const PREVIEW: Self = Self {
        preview: true,
        ..Self::NONE
    };

    pub(crate) const FULL: Self = Self {
        prompt: true,
        list: true,
        status: true,
        preview: true,
    };
}

impl BitOr for Redraw {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self {
            prompt: self.prompt | rhs.prompt,
            list: self.list | rhs.list,
            status: self.status | rhs.status,
            preview: self.preview | rhs.preview,
        }
    }
}

impl BitOrAssign for Redraw {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}
