use nucleo_picker_core::session::{Context, Exit};

use crate::Interface;

/// The single-selection picker interface.
///
/// This is essentially a minimal [`Interface`] implementation which only allows selection of a single item and
/// has no preview pane.
pub struct Single;

impl<T: Send + Sync + 'static> Interface<T> for Single {
    type State = ();
    type Output<'a> = Option<&'a T>;

    #[inline]
    fn init(&mut self, _cx: &Context<'_, T>) {}

    #[inline]
    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        match exit {
            Exit::Quit | Exit::Selected(None) => None,
            Exit::Selected(Some(id)) => {
                let item = cx
                    .matches()
                    .item(id)
                    .expect("selected item is not in the output context");
                Some(item.value())
            }
        }
    }
}
