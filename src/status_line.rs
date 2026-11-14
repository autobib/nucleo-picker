use std::{io, num::NonZero};

use crossterm::style::{Attribute, Color};
use unicode_width::UnicodeWidthChar;

use crate::{
    MatchStatus, PickerChars,
    component::Component,
    rect::{Area, Rect},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct StatusValues {
    matched: u32,
    total: u32,
    multi: Option<(u32, Option<NonZero<u32>>)>,
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
        multi: Option<(u32, Option<NonZero<u32>>)>,
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

impl Component<()> for StatusLine<'_> {
    fn resize(&mut self, _area: Area, _engine: &()) {}
    fn draw<D: Rect>(&mut self, _engine: &(), rect: &mut D) -> io::Result<()> {
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
    multi: Option<(u32, Option<NonZero<u32>>)>,
    status_marker: Option<char>,
    chars: &PickerChars,
) -> io::Result<()> {
    let width = rect.width().get();

    let mut occupied = status_marker.unwrap_or(' ').width().unwrap_or(0)
        + 1
        + decimal_width(matched)
        + 1
        + decimal_width(total);
    if let Some((ct, op)) = multi {
        occupied += 2 + decimal_width(ct) + 1;
        if let Some(max) = op {
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
    if let Some((ct, op)) = multi {
        rect.set_foreground(Color::Grey)?;
        rect.print(" (")?;
        rect.print(ct)?;
        if let Some(max) = op {
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
mod tests {
    use crate::{
        PickerChars,
        rect::{ClearState, CrosstermRect},
    };

    use super::*;

    fn rendered_prefix(width: u16, status_marker: Option<char>, clear: ClearState) -> String {
        let mut output = Vec::new();
        draw_match_counts(
            &mut CrosstermRect::new(
                &mut output,
                Area {
                    width,
                    height: 1,
                    ..Area::default()
                },
                clear,
            )
            .unwrap(),
            3,
            5,
            None,
            status_marker,
            &PickerChars::new(),
        )
        .unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn status_counts_and_markers_fit_or_clear_the_owned_width() {
        for (width, marker, clear, expected) in [
            (12, None, ClearState::ToEndOfLine, "  3/5"),
            (12, Some('⠏'), ClearState::ToEndOfLine, "⠏ 3/5"),
            (12, Some('≈'), ClearState::ToEndOfLine, "≈ 3/5"),
            (5, None, ClearState::ToEndOfLine, "  3/5"),
            (4, None, ClearState::ToEndOfLine, "\x1b[1G\x1b[K"),
            (4, None, ClearState::WithinRect, "\x1b[1G    \x1b[1G"),
        ] {
            let output = rendered_prefix(width, marker, clear);
            assert!(
                output.contains(expected),
                "width={width}, marker={marker:?}: {output:?}"
            );
            assert_eq!(output.contains("3/5"), width >= 5);
            assert_eq!(output.contains("─────"), width == 12);
            assert!(!output.contains("\x1b[2K"));
            if width < 5 {
                assert_eq!(output, expected);
            }
        }

        assert_eq!(decimal_width(0), 1);
        assert_eq!(decimal_width(9), 1);
        assert_eq!(decimal_width(10), 2);
        assert_eq!(decimal_width(u32::MAX), 10);
    }

    #[test]
    fn status_redraws_follow_counts_instead_of_list_changes() {
        use crate::{
            Picker,
            event::MatchListEvent,
            match_list::{MatchList, Queued, SelectedIndices},
            render::StrRenderer,
            session::Context,
        };

        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b"]);
        while picker.core.engine.update(5).matching {}
        picker.core.list_state.layout.update_items(
            picker.core.engine.snapshot(),
            &picker.core.list_state.config,
        );
        let mut list = MatchList::new(
            &mut picker.core.list_state,
            &picker.core.chars,
            SelectedIndices::default(),
        );
        let count = |list: &MatchList<'_, SelectedIndices>| {
            list.queued()
                .count()
                .map(|count| (count as u32, list.queued().limit()))
        };
        let mut status = StatusLine::new(&picker.core.chars);
        assert!(status.update(picker.core.engine.update(5), count(&list), false));

        use MatchListEvent::*;
        for (name, events, selected, changed) in [
            ("navigate", [Some(Up(1)), None, None], 0, false),
            (
                "toggle twice",
                [Some(ToggleDown(0)), Some(ToggleDown(0)), None],
                0,
                false,
            ),
            ("queue", [Some(ToggleDown(0)), None, None], 1, true),
            (
                "replace queued item",
                [Some(UnqueueAll), Some(Down(1)), Some(ToggleDown(0))],
                1,
                false,
            ),
        ] {
            for event in events.into_iter().flatten() {
                list.handle(event, &Context::new(&picker.core.engine));
            }
            list.flush(&picker.core.engine);
            let matching = picker.core.engine.update(5);
            assert!(list.update(&picker.core.engine, matching.items_changed, false));
            assert_eq!(
                status.update(matching, count(&list), false),
                changed,
                "{name}"
            );
            assert_eq!(list.queued().count(), Some(selected));
        }

        picker.core.engine.reparse("a");
        let matching = loop {
            let matching = picker.core.engine.update(5);
            if !matching.matching {
                break matching;
            }
        };
        assert!(status.update(matching, count(&list), false));
        let values = status.previous.unwrap();
        assert_eq!((values.matched, values.total), (1, 2));
        assert!(!status.update(matching, count(&list), false));
    }

    #[test]
    fn status_marker_updates_are_consumed_before_drawing() {
        let chars = PickerChars {
            spinner_chars: &['a', 'b'],
            matching_indicator: 'm',
            ..PickerChars::new()
        };
        let mut status = StatusLine::new(&chars);
        let mut matching = MatchStatus {
            items_changed: false,
            matched_item_count: 0,
            item_count: 0,
            matching: false,
            injecting: false,
        };
        assert!(status.update(matching, None, false));
        assert!(!status.update(matching, None, false));
        assert_eq!(status.marker.marker, None);
        for (is_matching, injecting, marker) in [
            (true, true, Some('a')),
            (true, true, Some('b')),
            (true, false, Some('m')),
            (false, false, None),
        ] {
            matching.matching = is_matching;
            matching.injecting = injecting;
            assert!(!status.update(matching, None, false));
            assert!(status.update(matching, None, true));
            assert_eq!(status.marker.marker, marker);
            assert!(!status.update(matching, None, false));
        }

        let area = Area {
            width: 20,
            height: 1,
            ..Area::default()
        };
        status.resize(area, &());
        let mut output = Vec::new();
        status
            .draw(
                &(),
                &mut CrosstermRect::new(&mut output, area, ClearState::Precleared).unwrap(),
            )
            .unwrap();
        assert!(String::from_utf8(output).unwrap().contains("0/0"));
        assert!(!status.update(matching, None, false));
    }
}
