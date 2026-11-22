use super::{Attributes, Color};

/// A complete drawing style.
///
/// Missing colors use terminal defaults and attributes which are not specified are disabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Style {
    /// The foreground color.
    pub foreground_color: Option<Color>,
    /// The background color.
    pub background_color: Option<Color>,
    /// The underline color (independent of enabling underlining).
    pub underline_color: Option<Color>,
    /// The set of active attributes.
    pub attributes: Attributes,
}

/// Styled borrowed text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyledStr<'a> {
    /// The borrowed text.
    pub text: &'a str,
    /// The style.
    pub style: Style,
}

macro_rules! attribute_method {
    ($($method:ident => $attribute:ident),* $(,)?) => {
        impl Style {
            $(
                #[doc = concat!("Add the ", stringify!($attribute), " attribute.")]
                #[inline]
                pub const fn $method(self) -> Self {
                    self.add_attributes(Attributes::$attribute)
                }
            )*
        }
    };
}

attribute_method! {
    bold => BOLD,
    dim => DIM,
    italic => ITALIC,
    underlined => UNDERLINED,
    reverse => REVERSE,
    hidden => HIDDEN,
    crossed_out => CROSSED_OUT,
    slow_blink => SLOW_BLINK,
    rapid_blink => RAPID_BLINK,
}

impl Style {
    /// The default style, but as a `const fn`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            foreground_color: None,
            background_color: None,
            underline_color: None,
            attributes: Attributes::empty(),
        }
    }

    /// Set the foreground color.
    #[inline]
    pub const fn fg(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    /// Set the background color.
    #[inline]
    pub const fn bg(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Set the underline color (without enabling underlining).
    #[inline]
    pub const fn underline_color(mut self, color: Color) -> Self {
        self.underline_color = Some(color);
        self
    }

    /// Replace the attribute set with a new one.
    #[inline]
    pub const fn attributes(mut self, attributes: Attributes) -> Self {
        self.attributes = attributes;
        self
    }

    /// Add attributes to the attribute set.
    #[inline]
    pub const fn add_attributes(mut self, attributes: Attributes) -> Self {
        self.attributes.insert(attributes);
        self
    }

    /// Remove attributes from the attribute set.
    #[inline]
    pub const fn remove_attributes(mut self, attributes: Attributes) -> Self {
        self.attributes.remove(attributes);
        self
    }

    /// Apply this style to borrowed text.
    #[inline]
    pub const fn apply(self, text: &str) -> StyledStr<'_> {
        StyledStr { text, style: self }
    }
}

#[cfg(test)]
mod tests;
