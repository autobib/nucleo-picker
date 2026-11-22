//! # Terminal frame
//!
//! The terminal frame is the entire contents of the terminal interface.
use crate::{
    Render, Terminal,
    interface::Interface,
    preview::BoundaryChars,
    rect::{Area, ClearState, CrosstermTarget, Dimensions},
    session::{Components, PreparedFrame},
};
use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};
use std::io;

#[derive(Clone, Copy)]
pub(crate) struct LayoutConfig {
    pub(crate) reversed: bool,
    pub(crate) preview_ratio: f64,
}

impl LayoutConfig {
    pub(crate) const fn new() -> Self {
        Self {
            reversed: false,
            preview_ratio: 0.5,
        }
    }

    pub(crate) const fn set_preview_split(&mut self, ratio: f64) {
        let valid = ratio >= 0.0 && ratio <= 1.0;
        debug_assert!(valid, "preview ratio must be in 0..=1");
        self.preview_ratio = if valid { ratio } else { 0.5 };
    }
}

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
    TogglePreview,
    /// Move the divider between the match list and the previewer to the left.
    MoveDividerLeft(u16),
    /// Move the divider between the match list and the previewer to the right.
    MoveDividerRight(u16),
}

/// The entire terminal frame.
///
/// The frame consists of sub-areas for the match list, prompt, status, and preview.
pub(crate) struct Layout {
    /// The dimensions of the frame.
    dimensions: Dimensions,
    reversed: bool,
    preview_ratio: f64,
    prompt: Area,
    list: Area,
    status: Area,
    preview: Area,
}

impl Layout {
    /// initialize a new frame.
    pub fn new(
        dimensions: Dimensions,
        reversed: bool,
        preview_ratio: f64,
        visible: Components,
    ) -> Self {
        let mut frame = Self {
            dimensions,
            reversed,
            preview_ratio,
            prompt: Area::default(),
            list: Area::default(),
            status: Area::default(),
            preview: Area::default(),
        };
        frame.layout(visible);
        frame
    }

    pub fn shift_preview(&mut self, columns: i32, visible: Components) -> bool {
        if self.preview.is_empty() {
            return false;
        }
        let width = i32::from(self.preview.width)
            .saturating_add(columns)
            .clamp(3, i32::from(self.dimensions.width) - 3) as u16;
        if width == self.preview.width {
            return false;
        }
        self.preview_ratio = f64::from(width) / f64::from(self.dimensions.width);
        self.layout(visible);
        true
    }

    /// Recompute the areas based on the current size.
    fn layout(&mut self, visible: Components) {
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
        self.preview = if visible.preview && width >= 6 && height >= 3 {
            let preview_width =
                ((self.preview_ratio * f64::from(width)).round() as u16).clamp(3, width - 3);
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
        let prompt_height = u16::from(visible.prompt);
        self.prompt = visible_area(
            Area {
                column: 0,
                row: if self.reversed {
                    0
                } else {
                    height.saturating_sub(1)
                },
                width,
                height: height.min(prompt_height),
            },
            visible.prompt,
        );
        let reserved_height = prompt_height + u16::from(visible.status);
        self.list = visible_area(
            Area {
                column: 0,
                row: if self.reversed {
                    height.min(reserved_height)
                } else {
                    0
                },
                width,
                height: height.saturating_sub(reserved_height),
            },
            visible.list,
        );
        self.status = visible_area(
            Area {
                column: 0,
                row: if self.reversed {
                    height.min(prompt_height)
                } else {
                    height.saturating_sub(reserved_height)
                },
                width,
                height: u16::from(height > prompt_height),
            },
            visible.status,
        );
    }

    /// Resolve the geometry of the standard layout without resizing the underlying components.
    ///
    /// Return `true` if the terminal dimensions or any component rectangle changed.
    pub fn resolve(&mut self, dimensions: Dimensions, visible: Components) -> bool {
        let previous = (
            self.dimensions,
            self.prompt,
            self.list,
            self.status,
            self.preview,
        );
        self.dimensions = dimensions;
        self.layout(visible);
        previous
            != (
                self.dimensions,
                self.prompt,
                self.list,
                self.status,
                self.preview,
            )
    }

    pub fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
}

/// Draw the requested components, or clear the screen and repaint all visible components.
pub(crate) fn draw<T, R, I, W>(
    layout: &Layout,
    frame: &mut PreparedFrame<'_, '_, T, R, I>,
    writer: &mut W,
    mut redraw: Components,
    clear_screen: bool,
    preview_boundary_chars: BoundaryChars,
) -> io::Result<()>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
    W: Terminal,
{
    let visible = frame.visible_components();
    let empty = layout.dimensions.width == 0 || layout.dimensions.height == 0;
    let clear = if clear_screen {
        redraw = Components::ALL;
        ClearState::Precleared
    } else if layout.preview.is_empty() {
        ClearState::ToEndOfLine
    } else {
        ClearState::WithinRect
    };
    redraw = redraw & visible;
    if !empty {
        writer.begin_render()?;
        writer.queue(BeginSynchronizedUpdate)?;
        if clear == ClearState::Precleared {
            writer
                .queue(ResetColor)?
                .queue(SetAttribute(Attribute::Reset))?
                .queue(Clear(ClearType::All))?;
        }
    }
    let mut target = CrosstermTarget::new(writer);
    if redraw.list || !visible.list || layout.list.is_empty() {
        frame
            .match_list()
            .draw(&mut target.rect(layout.list, clear))?;
    }
    if redraw.status || !visible.status || layout.status.is_empty() {
        frame
            .status()
            .draw(&mut target.rect(layout.status, clear))?;
    }
    if redraw.prompt || !visible.prompt || layout.prompt.is_empty() {
        frame
            .prompt()
            .draw(&mut target.rect(layout.prompt, clear))?;
    }
    if redraw.preview || !visible.preview || layout.preview.is_empty() {
        frame
            .preview()
            .border(preview_boundary_chars)
            .draw(&mut target.rect(layout.preview, clear))?;
    }
    if empty {
        return Ok(());
    }
    target.reset_style()?;
    if visible.prompt
        && !layout.prompt.is_empty()
        && let Some(cursor) = frame.prompt().cursor()
    {
        writer.queue(MoveTo(
            layout.prompt.column + cursor.column,
            layout.prompt.row + cursor.row,
        ))?;
    }
    writer.queue(EndSynchronizedUpdate)?;
    writer.end_render()
}

fn visible_area(area: Area, visible: bool) -> Area {
    if visible { area } else { Area::default() }
}

#[cfg(test)]
mod tests;
