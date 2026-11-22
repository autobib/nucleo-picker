use crate::{
    PickerChars,
    component::Component,
    draw::{Color, Style, Surface},
    session::{MatchStatus, QueueStatus},
    util::unicode::grapheme_width,
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
    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        if surface.is_empty() {
            return Ok(());
        }
        let values = self.previous.unwrap_or_default();
        draw_match_counts(
            surface,
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

fn draw_match_counts<S: Surface>(
    surface: &mut S,
    matched: u32,
    total: u32,
    multi: Option<QueueStatus>,
    status_marker: Option<char>,
    chars: &PickerChars,
) -> Result<(), S::Error> {
    let width = surface.width();

    let mut encoded = [0; 4];
    let marker = status_marker.unwrap_or(' ').encode_utf8(&mut encoded);
    let mut occupied =
        grapheme_width(marker) + 1 + decimal_width(matched) + 1 + decimal_width(total);
    if let Some(QueueStatus { count, limit }) = multi {
        occupied += 2 + decimal_width(count) + 1;
        if let Some(max) = limit {
            occupied += 1 + decimal_width(max.get());
        }
    }

    if occupied > usize::from(width) {
        return surface.clear_line();
    }

    let counts = Style::new().italic().fg(Color::Green);
    let secondary = Style::new().italic().fg(Color::Grey);
    surface.text(marker, counts)?;
    surface.ascii(format_args!(" {matched}/{total}"), counts)?;
    if let Some(QueueStatus { count, limit }) = multi {
        surface.ascii(format_args!(" ({count}"), secondary)?;
        if let Some(max) = limit {
            surface.ascii(format_args!("/{max}"), secondary)?;
        }
        surface.text(")", secondary)?;
    }

    let fill_width = usize::from(width).saturating_sub(occupied);
    if fill_width != 0 {
        surface.spaces(1, secondary)?;
        let mut encoded = [0; 4];
        let separator = chars.separator.encode_utf8(&mut encoded);
        for _ in 1..fill_width {
            surface.text(separator, secondary)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
