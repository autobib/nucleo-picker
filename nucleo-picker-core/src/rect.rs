/// The dimensions of a rectangle in the terminal, measured in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dimensions {
    /// The width of the rectangle in terminal columns.
    pub width: u16,
    /// The height of the rectangle in terminal columns.
    pub height: u16,
}

/// A zero-based cursor position relative to a rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// The zero-based terminal column (0 being at the top).
    pub column: u16,
    /// The zero-based terminal row (0 being at the left).
    pub row: u16,
}
