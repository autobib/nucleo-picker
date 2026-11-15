//! # Terminal frame
//!
//! The terminal frame is the entire contents of the terminal interface.
use crate::{
    Terminal,
    component::Component,
    rect::{Area, ClearState, CrosstermRect, Dimensions, Rect},
    session::Redraw,
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
    /// Toggle visibility of the status line.
    ///
    /// This event has no default keybinding.
    ToggleStatus,
    /// Toggle visibility of the preview pane.
    ///
    /// When the preview is hidden, the preview cache is preserved but events which target
    /// the preview have no effect.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    TogglePreview,
    /// Move the divider between the match list and the previewer to the left.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    MoveDividerLeft(u16),
    /// Move the divider between the match list and the previewer to the right.
    #[cfg(feature = "preview")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
    MoveDividerRight(u16),
}

pub(crate) trait Contents {
    fn prompt(&mut self) -> impl Component;
    fn list(&mut self) -> impl Component;
    fn status(&mut self) -> impl Component;
    fn preview(&mut self) -> impl Component;
    fn preview_resize(&mut self, area: Option<Area>);
}

#[cfg(test)]
impl<P: Component, L: Component, S: Component, V: Component> Contents for (P, L, S, V) {
    fn prompt(&mut self) -> impl Component {
        &mut self.0
    }
    fn list(&mut self) -> impl Component {
        &mut self.1
    }
    fn status(&mut self) -> impl Component {
        &mut self.2
    }
    fn preview(&mut self) -> impl Component {
        &mut self.3
    }
    fn preview_resize(&mut self, area: Option<Area>) {
        self.3.resize(area.unwrap_or_default());
    }
}

/// The entire terminal frame.
///
/// The frame consists of sub-areas for the match list, prompt, status, and preview.
pub(crate) struct Frame {
    dimensions: Dimensions,
    reversed: bool,
    status_hidden: bool,
    preview_ratio: Option<f64>,
    preview_hidden: bool,
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
    pub fn new(dimensions: Dimensions, reversed: bool, preview_ratio: Option<f64>) -> Self {
        let mut frame = Self {
            dimensions,
            reversed,
            status_hidden: false,
            preview_ratio,
            preview_hidden: false,
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
    pub fn dimensions(&self) -> Dimensions {
        self.dimensions
    }

    pub fn preview_enabled(&self) -> bool {
        self.preview_ratio.is_some() && !self.preview_hidden
    }

    pub fn status_enabled(&self) -> bool {
        !self.status_hidden
    }

    pub fn handle(&mut self, event: LayoutEvent) {
        match event {
            LayoutEvent::ToggleStatus => {
                self.status_hidden.toggle();
                self.layout();
                // TODO: maybe limit redraws only to match list / status line instead
                self.pending_resize = true;
            }
            #[cfg(feature = "preview")]
            LayoutEvent::TogglePreview => {
                if self.preview_ratio.is_some() {
                    self.preview_hidden.toggle();
                    self.layout();
                    self.pending_resize = true;
                }
            }
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
    fn update_dimensions(&mut self, dimensions: Dimensions) -> bool {
        if dimensions == self.dimensions {
            return false;
        }
        self.dimensions = dimensions;
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
            .clamp(3, i32::from(self.dimensions.width) - 3) as u16;
        if width == self.preview.width {
            return false;
        }
        self.preview_ratio = Some(f64::from(width) / f64::from(self.dimensions.width));
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
        let Dimensions { mut width, height } = self.dimensions;
        self.preview = if let Some(ratio) = self.preview_ratio
            && !self.preview_hidden
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
        let reserved_height = 1 + u16::from(!self.status_hidden);
        self.list = Area {
            column: 0,
            row: if self.reversed {
                height.min(reserved_height)
            } else {
                0
            },
            width,
            height: height.saturating_sub(reserved_height),
        };
        self.status = Area {
            column: 0,
            row: if self.reversed {
                height.min(1)
            } else {
                height.saturating_sub(2)
            },
            width,
            height: u16::from(!self.status_hidden && height >= 2),
        };
    }

    /// Resize all of the frame components.
    pub fn resize(&mut self, contents: &mut impl Contents) {
        contents.prompt().resize(self.prompt);
        contents.list().resize(self.list);
        contents.status().resize(self.status);
        contents.preview_resize(self.preview_enabled().then_some(self.preview));
        self.pending_resize = false;
    }

    /// Check terminal size, resize components, and perform necessary redraws.
    ///
    /// Returns whether a redraw actually occurred.
    pub fn render<W: Terminal>(
        &mut self,
        contents: &mut impl Contents,
        writer: &mut W,
        mut redraw: Redraw,
    ) -> io::Result<bool> {
        if redraw == Redraw::NONE && !self.pending_resize {
            // we don't need to check for a resize in this path since the resize
            // should have been propagated by a Redraw event
            return Ok(false);
        }
        self.pending_resize |= self.update_dimensions(writer.dimensions()?);
        if self.pending_resize {
            // resize immediately before drawing, so that the draw can take into account the new
            // size
            self.resize(contents);
            redraw = Redraw::FULL;
        }
        self.draw(contents, writer, redraw)?;
        Ok(true)
    }

    /// Draw the contents of the frame.
    pub fn draw<W: Terminal>(
        &self,
        contents: &mut impl Contents,
        writer: &mut W,
        mut redraw: Redraw,
    ) -> io::Result<()> {
        if self.dimensions.width == 0 || self.dimensions.height == 0 {
            return Ok(());
        }
        let clear = if redraw.prompt && redraw.list && redraw.status {
            redraw = Redraw::FULL;
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
            draw_component(&mut contents.list(), writer, self.list, clear)?;
        }
        if redraw.status {
            draw_component(&mut contents.status(), writer, self.status, clear)?;
        }
        if redraw.prompt {
            draw_component(&mut contents.prompt(), writer, self.prompt, clear)?;
        }
        if redraw.preview {
            draw_component(&mut contents.preview(), writer, self.preview, clear)?;
        }
        if let Some(cursor) = contents.prompt().cursor() {
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
fn draw_component<C: Component, W: io::Write + ?Sized>(
    component: &mut C,
    writer: &mut W,
    area: Area,
    clear: ClearState,
) -> io::Result<()> {
    let Some(mut rect) = CrosstermRect::new(writer, area, clear) else {
        return Ok(());
    };
    rect.reset_style()?;
    rect.move_to(0, 0)?;
    component.draw(&mut rect)?;
    rect.reset_style()
}

#[cfg(test)]
mod tests;
