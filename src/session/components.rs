use std::ops::{BitAnd, BitOr, BitOrAssign};

/// Components in in the picker interface.
///
/// The picker interface is composed of four components: the prompt, match list, status bar, and
/// preview. This struct is used whenever it is necessary to refer to a set of components.
///
/// # Convenience constants
///
/// There are four convenience constants
///
/// - `NONE`: no components
/// - `QUEUE`: the match list and status bar
/// - `PREVIEW`: the preview pane
/// - `ALL`: all components
///
/// Use `|`, `&`, and `|=` to combine component lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Components {
    /// The prompt.
    pub prompt: bool,
    /// The match list.
    pub list: bool,
    /// The status bar.
    pub status: bool,
    /// The preview pane.
    pub preview: bool,
}

impl Components {
    /// No components.
    pub const NONE: Self = Self {
        prompt: false,
        list: false,
        status: false,
        preview: false,
    };

    /// The match list and status bar.
    pub const QUEUE: Self = Self {
        list: true,
        status: true,
        ..Self::NONE
    };

    /// The preview pane.
    pub const PREVIEW: Self = Self {
        preview: true,
        ..Self::NONE
    };

    /// All components.
    pub const ALL: Self = Self {
        prompt: true,
        list: true,
        status: true,
        preview: true,
    };

    /// Returns if no components are included.
    ///
    /// This is the same as `components == Components::NONE`.
    #[inline]
    pub const fn is_none(self) -> bool {
        !(self.prompt | self.list | self.status | self.preview)
    }

    /// Returns if all components are included.
    ///
    /// This is the same as `components == Components::ALL`.
    #[inline]
    pub const fn is_all(self) -> bool {
        self.prompt & self.list & self.status & self.preview
    }
}

impl BitOr for Components {
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

impl BitOrAssign for Components {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl BitAnd for Components {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self {
            prompt: self.prompt & rhs.prompt,
            list: self.list & rhs.list,
            status: self.status & rhs.status,
            preview: self.preview & rhs.preview,
        }
    }
}
