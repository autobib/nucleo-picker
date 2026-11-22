pub(crate) fn write_spaces<W: std::io::Write + ?Sized>(
    writer: &mut W,
    mut width: usize,
) -> std::io::Result<()> {
    const SPACES: &[u8] = b"                                                                ";

    while width >= SPACES.len() {
        writer.write_all(SPACES)?;
        width -= SPACES.len();
    }
    writer.write_all(&SPACES[..width])
}
