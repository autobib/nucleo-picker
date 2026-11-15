use std::io;

use crossterm::style::{Attribute, Color};
use unicode_width::UnicodeWidthChar;

use crate::{
    PickerChars,
    component::Component,
    rect::{Area, Rect},
    session::{MatchStatus, QueueStatus},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct StatusValues {
    matched: u32,
    total: u32,
    multi: Option<QueueStatus>,
    marker: Option<char>,
}

#[derive(Default)]
struct StatusMarker {
    displayed_injecting: bool,
    marker: Option<char>,
    spinner_index: usize,
}

impl StatusMarker {
    fn update(&mut self, matching: bool, injecting: bool, chars: &PickerChars) {
        if injecting && self.displayed_injecting {
            if !chars.spinner_chars.is_empty() {
                self.spinner_index = (self.spinner_index + 1) % chars.spinner_chars.len();
            }
        } else {
            self.spinner_index = 0;
        }
        let marker = if injecting {
            chars.spinner_chars.get(self.spinner_index).copied()
        } else {
            matching.then_some(chars.matching_indicator)
        };
        self.marker = marker;
        self.displayed_injecting = injecting;
    }
}

/// The status line.
pub(crate) struct StatusLine<'a> {
    chars: &'a PickerChars,
    // cache of the previous values, to decide if the status needs to be redrawn
    previous: Option<StatusValues>,
    marker: StatusMarker,
}

impl<'a> StatusLine<'a> {
    pub fn new(chars: &'a PickerChars) -> Self {
        Self {
            chars,
            previous: None,
            marker: StatusMarker::default(),
        }
    }

    pub fn update(
        &mut self,
        status: MatchStatus,
        multi: Option<QueueStatus>,
        background_frame: bool,
    ) -> bool {
        if background_frame {
            self.marker
                .update(status.matching, status.injecting, self.chars);
        }
        let values = StatusValues {
            matched: status.matched_item_count,
            total: status.item_count,
            multi,
            marker: self.marker.marker,
        };
        let changed = self.previous != Some(values);
        self.previous = Some(values);
        changed
    }
}

impl Component for StatusLine<'_> {
    fn resize(&mut self, _area: Area) {}
    fn draw<D: Rect>(&mut self, rect: &mut D) -> io::Result<()> {
        let values = self.previous.unwrap_or_default();
        draw_match_counts(
            rect,
            values.matched,
            values.total,
            values.multi,
            values.marker,
            self.chars,
        )
    }
}

fn decimal_width(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
}

fn draw_match_counts<D: Rect>(
    rect: &mut D,
    matched: u32,
    total: u32,
    multi: Option<QueueStatus>,
    status_marker: Option<char>,
    chars: &PickerChars,
) -> io::Result<()> {
    let width = rect.width().get();

    let mut occupied = status_marker.unwrap_or(' ').width().unwrap_or(0)
        + 1
        + decimal_width(matched)
        + 1
        + decimal_width(total);
    if let Some(QueueStatus { count, limit }) = multi {
        occupied += 2 + decimal_width(count) + 1;
        if let Some(max) = limit {
            occupied += 1 + decimal_width(max.get());
        }
    }

    if occupied > usize::from(width) {
        return rect.clear_line();
    }

    rect.set_attribute(Attribute::Italic)?;
    rect.set_foreground(Color::Green)?;
    rect.print(status_marker.unwrap_or(' '))?;
    rect.print(" ")?;
    rect.print(matched)?;
    rect.print("/")?;
    rect.print(total)?;
    if let Some(QueueStatus { count, limit }) = multi {
        rect.set_foreground(Color::Grey)?;
        rect.print(" (")?;
        rect.print(count)?;
        if let Some(max) = limit {
            rect.print("/")?;
            rect.print(max)?;
        }
        rect.print(")")?;
    }

    let fill_width = usize::from(width).saturating_sub(occupied);
    if fill_width != 0 {
        rect.set_foreground(Color::Grey)?;
        rect.print(" ")?;
        for _ in 1..fill_width {
            rect.print(chars.separator)?;
        }
    }
    rect.reset_style()
}

#[cfg(test)]
mod tests;
