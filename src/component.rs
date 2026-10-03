use std::{ops::BitOrAssign, time::Instant};

use crate::match_list::MatchList;
#[cfg(feature = "preview")]
use crate::preview::PreviewEvent;

pub trait ComponentStatus: BitOrAssign + Default {
    fn needs_redraw(&self) -> bool;
}

impl ComponentStatus for bool {
    fn needs_redraw(&self) -> bool {
        *self
    }
}

/// A helper trait for `pick_impl` to be generic over both a previewer and the 'null previewer'
/// `()`.
pub(crate) trait PreviewComponent<T: Send + Sync + 'static, R, A> {
    fn update(&mut self, matches: &MatchList<T, R>, deadline: Instant) -> Result<(), A>;

    fn restart(&mut self);

    #[cfg(feature = "preview")]
    fn scroll(&mut self, _idx: Option<u32>, _event: PreviewEvent, _height: u16) -> bool {
        false
    }

    #[cfg(feature = "preview")]
    fn draw(&mut self, _matches: &MatchList<T, R>, _height: u16) {}
}

impl<T: Send + Sync + 'static, R, A> PreviewComponent<T, R, A> for () {
    fn update(&mut self, _: &MatchList<T, R>, _: Instant) -> Result<(), A> {
        Ok(())
    }

    fn restart(&mut self) {}
}
