use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Sub, SubAssign};

/// A set of attributes modifying terminal presentation of text.
///
/// Unlike crossterm styles, this omits reset attributes and therefore represents positive
/// attributes only. In particular, conversion from crossterm styles is lossy.
///
/// ## Attribute order
///
/// Attributes are emitted in the following order:
///
/// - [`Attribute::BOLD`]
/// - [`Attribute::DIM`]
/// - [`Attribute::ITALIC`]
/// - [`Attribute::UNDERLINED`]
/// - [`Attribute::DOUBLE_UNDERLINED`]
/// - [`Attribute::UNDERCURLED`]
/// - [`Attribute::UNDERDOTTED`]
/// - [`Attribute::UNDERDASHED`]
/// - [`Attribute::SLOW_BLINK`]
/// - [`Attribute::RAPID_BLINK`]
/// - [`Attribute::REVERSE`]
/// - [`Attribute::HIDDEN`]
/// - [`Attribute::CROSSED_OUT`]
/// - [`Attribute::FRAKTUR`]
/// - [`Attribute::FRAMED`]
/// - [`Attribute::ENCIRCLED`]
/// - [`Attribute::OVERLINED`]
///
/// In particular, for terminals that treat different attributes as mutually exclusive modes, later
/// attributes will typically take precedence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Attributes(u32);

macro_rules! attributes {
    ($($name:ident = $bit:literal),* $(,)?) => {
        impl Attributes {
            $(
                #[doc = concat!("The ", stringify!($name), " attribute.")]
                pub const $name: Self = Self(1 << $bit);
            )*
        }
    };
}

attributes! {
    BOLD = 1,
    DIM = 2,
    ITALIC = 3,
    UNDERLINED = 4,
    DOUBLE_UNDERLINED = 5,
    UNDERCURLED = 6,
    UNDERDOTTED = 7,
    UNDERDASHED = 8,
    SLOW_BLINK = 9,
    RAPID_BLINK = 10,
    REVERSE = 11,
    HIDDEN = 12,
    CROSSED_OUT = 13,
    FRAKTUR = 14,
    FRAMED = 23,
    ENCIRCLED = 24,
    OVERLINED = 25,
}

impl Attributes {
    /// An empty attribute set.
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns whether the attribute set is empty.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether all flags in `self` are present in `other`.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Takes the union of the attributes.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Takes the intersection of the attributes.
    #[inline]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Takes the set difference of the attributes.
    #[inline]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Insert attributes present in `other`.
    #[inline]
    pub const fn insert(&mut self, other: Self) {
        *self = self.union(other);
    }

    /// Remove attributes present in `other`.
    #[inline]
    pub const fn remove(&mut self, other: Self) {
        *self = self.difference(other);
    }
}

macro_rules! set_operator {
    ($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident, $operation:ident) => {
        impl $trait for Attributes {
            type Output = Self;

            #[inline]
            fn $method(self, rhs: Self) -> Self {
                self.$operation(rhs)
            }
        }

        impl $assign_trait for Attributes {
            #[inline]
            fn $assign_method(&mut self, rhs: Self) {
                *self = self.$operation(rhs);
            }
        }
    };
}

set_operator!(BitOr, bitor, BitOrAssign, bitor_assign, union);
set_operator!(BitAnd, bitand, BitAndAssign, bitand_assign, intersection);
set_operator!(Sub, sub, SubAssign, sub_assign, difference);
