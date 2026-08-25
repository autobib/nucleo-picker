use std::ops::BitOrAssign;

pub trait ComponentStatus: BitOrAssign + Default {
    fn needs_redraw(&self) -> bool;
}

impl ComponentStatus for bool {
    fn needs_redraw(&self) -> bool {
        *self
    }
}
