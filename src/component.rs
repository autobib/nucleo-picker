use std::{ops::BitOrAssign, time::Instant};

use crate::match_list::MatchList;
#[cfg(feature = "preview")]
use crate::preview::{Cached, PreviewEvent};

pub trait ComponentStatus: BitOrAssign + Default {
    fn needs_redraw(&self) -> bool;
}

impl ComponentStatus for bool {
    fn needs_redraw(&self) -> bool {
        *self
    }
}

/// A trait so that `pick_impl` can be generic over the (non-)existence of a previewer.
pub(crate) trait PreviewComponent<T: Send + Sync + 'static, R, A> {
    /// Update the preview pane to take into account changes to the match list.
    ///
    /// This method is called after event handling. The provided deadline is the frame deadline,
    /// which can be used to pass on a budget to a previewer.
    ///
    /// Return `true` if there are changes which require drawing, and `false` otherwise.
    fn update(&mut self, _matches: &MatchList<T, R>, _deadline: Instant) -> Result<bool, A> {
        Ok(false)
    }

    /// Handle a restart event.
    ///
    /// For example, the previewer may use this to evict items from the cache.
    fn restart(&mut self) {}

    /// Previewers may use this to adjust the starting index (for example, to avoid unnecessary
    /// whitespace at the bottom of the screen)
    fn resize(&mut self, _height: u16) {}

    /// Handle a preview event.
    ///
    /// The preview events are forwarded in the order in which they are received, and target the
    /// provided `idx`. Return `true` if there are changes which require drawing, and `false`
    /// otherwise.
    ///
    /// This method is not called if the preview frame has height 0.
    #[cfg(feature = "preview")]
    fn scroll(&mut self, _idx: Option<u32>, _event: PreviewEvent, _height: u16) -> bool {
        false
    }

    /// Return a cached preview for the renderer to draw
    #[cfg(feature = "preview")]
    fn cached(&self) -> Option<&Cached> {
        None
    }
}

impl<T: Send + Sync + 'static, R, A> PreviewComponent<T, R, A> for () {}
