use std::ops::BitOrAssign;

pub trait Status: BitOrAssign + Default {
    fn needs_redraw(&self) -> bool;
}

impl Status for bool {
    fn needs_redraw(&self) -> bool {
        *self
    }
}

pub trait Component {
    /// The event that this component can handle.
    type Event;

    /// The status of the component after handling an event, such as whether or not the component
    /// needs to be redrawn. Supports updating.
    type Status: Status;

    /// Update the component state in response to the given event, returning whether or not the
    /// component changed.
    fn handle(&mut self, event: Self::Event) -> Self::Status;

    /// Redraw the component in the screen. The cursor will be placed in the top-left corner of the
    /// provided region during redraw.
    fn draw<W: std::io::Write + ?Sized>(
        &mut self,
        width: u16,
        height: u16,
        writer: &mut W,
    ) -> std::io::Result<()>;
}
