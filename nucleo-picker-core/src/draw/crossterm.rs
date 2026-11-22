use crossterm::style::{
    Attribute, Attributes as CrosstermAttributes, Color as CrosstermColor, ContentStyle,
    StyledContent,
};

use super::{Attributes, Color, Style, StyledStr};

macro_rules! colors {
    ($($variant:ident $({ $($field:ident),* })? $(( $value:ident ))?),* $(,)?) => {
        impl From<Color> for CrosstermColor {
            #[inline]
            fn from(color: Color) -> Self {
                match color {
                    $(
                        Color::$variant $({ $($field),* })? $(($value))? =>
                            Self::$variant $({ $($field),* })? $(($value))?,
                    )*
                }
            }
        }

        impl From<CrosstermColor> for Color {
            #[inline]
            fn from(color: CrosstermColor) -> Self {
                match color {
                    $(
                        CrosstermColor::$variant $({ $($field),* })? $(($value))? =>
                            Self::$variant $({ $($field),* })? $(($value))?,
                    )*
                }
            }
        }
    };
}

colors! {
    Reset, Black, DarkGrey, Red, DarkRed, Green, DarkGreen, Yellow, DarkYellow,
    Blue, DarkBlue, Magenta, DarkMagenta, Cyan, DarkCyan, White, Grey,
    Rgb { r, g, b }, AnsiValue(value),
}

macro_rules! attributes {
    (
        $($native:ident => $crossterm:ident,)*
        ;
        $($off:ident => $($removed:ident)|+),* $(,)?
    ) => {
        impl From<Attributes> for CrosstermAttributes {
            #[inline]
            fn from(attributes: Attributes) -> Self {
                let mut converted = Self::none();
                $(
                    if attributes.contains(Attributes::$native) {
                        converted.set(Attribute::$crossterm);
                    }
                )*
                converted
            }
        }

        impl From<CrosstermAttributes> for Attributes {
            #[inline]
            fn from(attributes: CrosstermAttributes) -> Self {
                let mut converted = Self::empty();
                $(
                    if attributes.has(Attribute::$crossterm) {
                        converted.insert(Self::$native);
                    }
                )*
                $(
                    if attributes.has(Attribute::$off) {
                        converted.remove($(Self::$removed)|+);
                    }
                )*
                converted
            }
        }
    };
}

attributes! {
    BOLD => Bold,
    DIM => Dim,
    ITALIC => Italic,
    UNDERLINED => Underlined,
    DOUBLE_UNDERLINED => DoubleUnderlined,
    UNDERCURLED => Undercurled,
    UNDERDOTTED => Underdotted,
    UNDERDASHED => Underdashed,
    SLOW_BLINK => SlowBlink,
    RAPID_BLINK => RapidBlink,
    REVERSE => Reverse,
    HIDDEN => Hidden,
    CROSSED_OUT => CrossedOut,
    FRAKTUR => Fraktur,
    FRAMED => Framed,
    ENCIRCLED => Encircled,
    OVERLINED => OverLined,
    ;
    NoBold => BOLD,
    NormalIntensity => BOLD | DIM,
    NoItalic => ITALIC | FRAKTUR,
    NoUnderline => UNDERLINED | DOUBLE_UNDERLINED | UNDERCURLED | UNDERDOTTED | UNDERDASHED,
    NoBlink => SLOW_BLINK | RAPID_BLINK,
    NoReverse => REVERSE,
    NoHidden => HIDDEN,
    NotCrossedOut => CROSSED_OUT,
    NotFramedOrEncircled => FRAMED | ENCIRCLED,
    NotOverLined => OVERLINED,
}

impl From<Style> for ContentStyle {
    #[inline]
    fn from(style: Style) -> Self {
        Self {
            foreground_color: style.foreground_color.map(Into::into),
            background_color: style.background_color.map(Into::into),
            underline_color: style.underline_color.map(Into::into),
            attributes: style.attributes.into(),
        }
    }
}

impl From<ContentStyle> for Style {
    #[inline]
    fn from(style: ContentStyle) -> Self {
        let mut converted = Self {
            attributes: style.attributes.into(),
            ..Self::new()
        };
        if !style.attributes.has(Attribute::Reset) {
            converted.foreground_color = style.foreground_color.map(Into::into);
            converted.background_color = style.background_color.map(Into::into);
            converted.underline_color = style.underline_color.map(Into::into);
        }
        converted
    }
}

impl<'a> From<StyledStr<'a>> for StyledContent<&'a str> {
    #[inline]
    fn from(styled: StyledStr<'a>) -> Self {
        Self::new(styled.style.into(), styled.text)
    }
}

impl<'a> From<StyledContent<&'a str>> for StyledStr<'a> {
    #[inline]
    fn from(styled: StyledContent<&'a str>) -> Self {
        Self {
            text: *styled.content(),
            style: (*styled.style()).into(),
        }
    }
}

#[cfg(test)]
mod tests;
