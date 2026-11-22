/// Terminal colors.
///
/// This is basically a crossterm-compatible terminal color type, including the bright and dark
/// names and the distinction between named and indexed colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal default color.
    Reset,
    /// Black.
    Black,
    /// Dark grey.
    DarkGrey,
    /// Bright red.
    Red,
    /// Dark red.
    DarkRed,
    /// Bright green.
    Green,
    /// Dark green.
    DarkGreen,
    /// Bright yellow.
    Yellow,
    /// Dark yellow.
    DarkYellow,
    /// Bright blue.
    Blue,
    /// Dark blue.
    DarkBlue,
    /// Bright magenta.
    Magenta,
    /// Dark magenta.
    DarkMagenta,
    /// Bright cyan.
    Cyan,
    /// Dark cyan.
    DarkCyan,
    /// White.
    White,
    /// Grey.
    Grey,
    /// 24-bit RGB colors.
    Rgb {
        /// The red channel.
        r: u8,
        /// The green channel.
        g: u8,
        /// The blue channel.
        b: u8,
    },
    /// An index into the terminal's 256-color palette.
    AnsiValue(u8),
}
