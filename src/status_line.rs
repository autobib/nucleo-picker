use crossterm::style::{ContentStyle, Stylize};
use unicode_width::UnicodeWidthChar;

use crate::{
    PickerChars,
    component::Component,
    draw::Draw,
    rect::Area,
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
    fn draw<D: Draw>(&mut self, rect: &mut D) -> Result<(), D::Error> {
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

fn draw_match_counts<D: Draw>(
    rect: &mut D,
    matched: u32,
    total: u32,
    multi: Option<QueueStatus>,
    status_marker: Option<char>,
    chars: &PickerChars,
) -> Result<(), D::Error> {
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

    let counts = ContentStyle::new().italic().green();
    let secondary = ContentStyle::new().italic().grey();
    rect.text(
        status_marker.unwrap_or(' ').encode_utf8(&mut [0; 4]),
        counts,
    )?;
    rect.ascii(format_args!(" {matched}/{total}"), counts)?;
    if let Some(QueueStatus { count, limit }) = multi {
        rect.ascii(format_args!(" ({count}"), secondary)?;
        if let Some(max) = limit {
            rect.ascii(format_args!("/{max}"), secondary)?;
        }
        rect.text(")", secondary)?;
    }

    let fill_width = usize::from(width).saturating_sub(occupied);
    if fill_width != 0 {
        rect.spaces(1, secondary)?;
        let mut encoded = [0; 4];
        let separator = chars.separator.encode_utf8(&mut encoded);
        for _ in 1..fill_width {
            rect.text(separator, secondary)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
