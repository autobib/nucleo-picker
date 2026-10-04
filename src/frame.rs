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
    preview_enabled: bool,
    prompt: Area,
    list: Area,
    status: Area,
    preview: Area,
}

impl Frame {
    /// initialize a new frame.
    pub fn new(size: (u16, u16), reversed: bool, preview_enabled: bool) -> Self {
        let mut frame = Self {
            size,
            reversed,
            preview_enabled,
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

    /// Update the size of the frame, returning `true` if the frame size changed and a redraw is
    /// required.
    pub fn update_size(&mut self, size: (u16, u16)) -> bool {
        if size == self.size {
            return false;
        }
        self.size = size;
        self.layout();
        true
    }

    /// Recompute the areas based on the current size.
    fn layout(&mut self) {
        let (mut width, height) = self.size;
        // TODO: don't just hard-code this eventually
        let preview_width = width / 2;
        self.preview = if self.preview_enabled && preview_width >= 3 && height >= 3 {
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
        &self,
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
