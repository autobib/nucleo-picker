use super::Preview;
use crate::Picker;

/// A picker which also generates a preview pane for items.
pub struct PreviewPicker<'a, T: Send + Sync + 'static, R, P> {
    picker: &'a mut Picker<T, R>,
    previewer: P,
}

impl<'a, T: Send + Sync + 'static, R, P> PreviewPicker<'a, T, R, P> {
    /// Returns a reference to the internal picker.
    pub fn picker(&self) -> &Picker<T, R> {
        self.picker
    }

    /// Returns an exclusive reference to the internal picker.
    pub fn picker_mut(&mut self) -> &mut Picker<T, R> {
        self.picker
    }
}
