//! # Terminal frame
//!
//! The terminal frame is the entire contents of the terminal interface.
use crate::{
    Terminal,
    component::Component,
    rect::{Area, ClearState, CrosstermRect, Rect},
};
use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};
use std::io;

/// Events which modify the layout of the picker screen.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayoutEvent {
    /// Move the divider between the match list and the previewer to the left.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    MoveDividerLeft(u16),
    /// Move the divider between the match list and the previewer to the right.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    MoveDividerRight(u16),
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Redraw {
    pub prompt: bool,
    pub list: bool,
    pub status: bool,
    pub preview: bool,
}

impl Redraw {
    pub fn any(self) -> bool {
        self.prompt || self.list || self.status || self.preview
    }
    pub fn full() -> Self {
        Self {
            prompt: true,
            list: true,
            status: true,
            preview: true,
        }
    }
}

/// The entire terminal frame.
///
/// The frame consists of sub-areas for the match list, prompt, status, and preview.
pub(crate) struct Frame {
    size: (u16, u16),
    reversed: bool,
    preview_ratio: Option<f64>,
    pending_resize: bool,
    prompt: Area,
    list: Area,
    status: Area,
    preview: Area,
}

impl Frame {
    /// initialize a new frame.
    ///
    /// `preview_ratio = None` means that the preview pane is not displayed at all
    pub fn new(size: (u16, u16), reversed: bool, preview_ratio: Option<f64>) -> Self {
        let mut frame = Self {
            size,
            reversed,
            preview_ratio,
            pending_resize: false,
            prompt: Area::default(),
            list: Area::default(),
            status: Area::default(),
            preview: Area::default(),
        };
        frame.layout();
        frame
    }

    /// The dimensions of the frame.
    pub fn dimensions(&self) -> (u16, u16) {
        self.size
    }

    pub fn handle(&mut self, event: LayoutEvent) {
        match event {
            #[cfg(feature = "preview")]
            LayoutEvent::MoveDividerLeft(columns) => {
                self.pending_resize |= self.shift_preview(i32::from(columns));
            }
            #[cfg(feature = "preview")]
            LayoutEvent::MoveDividerRight(columns) => {
                self.pending_resize |= self.shift_preview(-i32::from(columns));
            }
        }
    }

    /// Update the size of the frame, returning `true` if the frame size changed and a redraw is
    /// required.
    fn update_size(&mut self, size: (u16, u16)) -> bool {
        if size == self.size {
            return false;
        }
        self.size = size;
        self.layout();
        true
    }

    #[cfg(feature = "preview")]
    fn shift_preview(&mut self, columns: i32) -> bool {
        if self.preview.is_empty() {
            return false;
        }
        let width = i32::from(self.preview.width)
            .saturating_add(columns)
            .clamp(3, i32::from(self.size.0) - 3) as u16;
        if width == self.preview.width {
            return false;
        }
        self.preview_ratio = Some(f64::from(width) / f64::from(self.size.0));
        self.layout();
        true
    }

    /// Recompute the areas based on the current size.
    fn layout(&mut self) {
        // resize policy for terminal width W:
        //
        // 1. round ratio * W to the nearest column, with ties rounded up
        // 2. clamp the preview width between 3 and W - 3 columns. the match list gets the remaining columns.
        // 3. preview ratios 0 and 1 request the smallest and largest usable preview sizes.
        // 4. below 6 terminal columns or 3 rows: hide the preview and give the picker the full area.
        // 5. terminal resizing never changes the stored ratio so that resizing can round-trip
        //    correctly.
        // 6. When moving the divider: start from the displayed preview width, clamp the adjusted width
        //    q as above, and store q / W as the new ratio iff the width changes (i.e: resizing divider
        //    incrementally causes ratio rounding)
        // 7. Adjustments which do not result in a change (e.g. against a limit) do not adjust the
        //    ratio.
        let (mut width, height) = self.size;
        self.preview = if let Some(ratio) = self.preview_ratio
            && width >= 6
            && height >= 3
        {
            let preview_width = ((ratio * f64::from(width)).round() as u16).clamp(3, width - 3);
            width -= preview_width;
            Area {
                column: width,
                row: 0,
                width: preview_width,
                height,
            }
        } else {
            Area::default()
        };
        self.prompt = Area {
            column: 0,
            row: if self.reversed {
                0
            } else {
                height.saturating_sub(1)
            },
            width,
            height: height.min(1),
        };
        self.list = Area {
            column: 0,
            row: if self.reversed { height.min(2) } else { 0 },
            width,
            height: height.saturating_sub(2),
        };
        self.status = Area {
            column: 0,
            row: if self.reversed {
                height.min(1)
            } else {
                height.saturating_sub(2)
            },
            width,
            height: u16::from(height >= 2),
        };
    }

    /// Resize all of the frame components.
    pub fn resize<S, P: Component<S>, L: Component<S>, C: Component<S>, V: Component<S>>(
        &mut self,
        engine: &S,
        prompt: &mut P,
        list: &mut L,
        status: &mut C,
        preview: &mut V,
    ) {
        prompt.resize(self.prompt, engine);
        list.resize(self.list, engine);
        status.resize(self.status, engine);
        preview.resize(self.preview, engine);
        self.pending_resize = false;
    }

    /// Render the frame to the screen if required.
    ///
    /// Redraws occur in one of the following cases cases:
    ///
    /// - a redraw was externally requested
    /// - either a component has requested a redraw because it changed
    /// - there is a layout change (handled internally)
    /// - the screen size changed (checked here, at the last possible moment, so rendering reflects
    ///   the acutal screen size)
    ///
    /// This method returns `Ok(true)` if a redraw actually occurred and `Ok(false)` otherwise.
    #[allow(clippy::too_many_arguments)]
    pub fn render<S, P, L, C, V, W>(
        &mut self,
        engine: &S,
        prompt: &mut P,
        list: &mut L,
        status: &mut C,
        preview: &mut V,
        writer: &mut W,
        mut redraw: Redraw,
    ) -> io::Result<bool>
    where
        P: Component<S>,
        L: Component<S>,
        C: Component<S>,
        V: Component<S>,
        W: Terminal,
    {
        if !redraw.any() && !self.pending_resize {
            // we don't need to check for a resize in this path since the resize
            // should have been propagated by a Redraw event
            return Ok(false);
        }
        self.pending_resize |= self.update_size(writer.size()?);
        if self.pending_resize {
            self.resize(engine, prompt, list, status, preview);
            redraw = Redraw::full();
        }
        self.draw(engine, prompt, list, status, preview, writer, redraw)?;
        Ok(true)
    }

    /// Draw the contents of the frame.
    #[allow(clippy::too_many_arguments)]
    pub fn draw<S, P, L, C, V, W>(
        &self,
        engine: &S,
        prompt: &mut P,
        list: &mut L,
        status: &mut C,
        preview: &mut V,
        writer: &mut W,
        mut redraw: Redraw,
    ) -> io::Result<()>
    where
        P: Component<S>,
        L: Component<S>,
        C: Component<S>,
        V: Component<S>,
        W: Terminal,
    {
        if self.size.0 == 0 || self.size.1 == 0 {
            return Ok(());
        }
        let clear = if redraw.prompt && redraw.list && redraw.status {
            redraw = Redraw::full();
            ClearState::Precleared
        } else if self.preview.is_empty() {
            ClearState::ToEndOfLine
        } else {
            ClearState::WithinRect
        };
        writer.begin_render()?;
        writer.queue(BeginSynchronizedUpdate)?;
        if clear == ClearState::Precleared {
            writer
                .queue(ResetColor)?
                .queue(SetAttribute(Attribute::Reset))?
                .queue(Clear(ClearType::All))?;
        }
        if redraw.list {
            draw_component(list, engine, writer, self.list, clear)?;
        }
        if redraw.status {
            draw_component(status, engine, writer, self.status, clear)?;
        }
        if redraw.prompt {
            draw_component(prompt, engine, writer, self.prompt, clear)?;
        }
        if redraw.preview {
            draw_component(preview, engine, writer, self.preview, clear)?;
        }
        if let Some(cursor) = prompt.cursor(engine) {
            writer.queue(MoveTo(
                self.prompt.column + cursor.column,
                self.prompt.row + cursor.row,
            ))?;
        }
        writer.queue(EndSynchronizedUpdate)?;
        writer.end_render()
    }
}

/// Draw a component to the screen in the provided area with the given clear policy.
fn draw_component<S, C: Component<S>, W: io::Write + ?Sized>(
    component: &mut C,
    engine: &S,
    writer: &mut W,
    area: Area,
    clear: ClearState,
) -> io::Result<()> {
    let Some(mut rect) = CrosstermRect::new(writer, area, clear) else {
        return Ok(());
    };
    rect.reset_style()?;
    rect.move_to(0, 0)?;
    component.draw(engine, &mut rect)?;
    rect.reset_style()
}

#[cfg(test)]
mod tests;
