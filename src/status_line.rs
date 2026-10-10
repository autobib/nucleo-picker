use std::{io, num::NonZero};

use crossterm::style::{Attribute, Color};
use unicode_width::UnicodeWidthChar;

use crate::{
    PickerChars,
    component::Component,
    match_engine::MatchStatus,
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
            matched: status.matched,
            total: status.total,
            multi,
            marker: self.marker.marker,
        };
        let changed = self.previous != Some(values);
        self.previous = Some(values);
        changed
    }
}

impl<E> Component<E> for StatusLine<'_> {
    fn resize(&mut self, _area: Area, _engine: &E) {}
    fn draw<D: Rect>(&mut self, _engine: &E, rect: &mut D) -> io::Result<()> {
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

    fn rendered_prefix(width: u16, status_marker: Option<char>) -> String {
        let mut output = Vec::new();
        draw_match_counts(
            &mut CrosstermRect::new(
                &mut output,
                Area {
                    width,
                    height: 1,
                    ..Area::default()
                },
                ClearState::ToEndOfLine,
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
    fn status_line() {
        assert!(rendered_prefix(12, None).contains("  3/5"));
        assert!(rendered_prefix(12, Some('⠏')).contains("⠏ 3/5"));
        assert!(rendered_prefix(12, Some('≈')).contains("≈ 3/5"));

        assert!(rendered_prefix(12, None).contains("─────"));

        assert!(!rendered_prefix(4, None).contains("3/5"));

        let exact = rendered_prefix(5, None);
        assert!(exact.contains("3/5"));
        assert!(!exact.contains("\x1b[2K"));

        assert_eq!(decimal_width(0), 1);
        assert_eq!(decimal_width(9), 1);
        assert_eq!(decimal_width(10), 2);
        assert_eq!(decimal_width(u32::MAX), 10);
    }

    #[test]
    fn exact_status_clear_fills_a_too_narrow_line() {
        let mut output = Vec::new();
        draw_match_counts(
            &mut CrosstermRect::new(
                &mut output,
                Area {
                    width: 4,
                    height: 1,
                    ..Area::default()
                },
                ClearState::WithinRect,
            )
            .unwrap(),
            3,
            5,
            None,
            None,
            &PickerChars::new(),
        )
        .unwrap();

        assert_eq!(output, b"\x1b[1G    \x1b[1G");
    }
    #[test]
    fn marker_cycles_while_injecting_then_indicates_matching() {
        let mut marker = StatusMarker::default();
        let chars = PickerChars {
            spinner_chars: &['a', 'b'],
            matching_indicator: 'm',
            ..PickerChars::new()
        };
        marker.update(true, true, &chars);
        assert_eq!(marker.marker, Some('a'));
        marker.update(true, true, &chars);
        assert_eq!(marker.marker, Some('b'));
        marker.update(true, false, &chars);
        assert_eq!(marker.marker, Some('m'));
        assert_eq!(StatusMarker::default().marker, None);
    }

    #[test]
    fn unchanged_counts_do_not_redraw_after_list_changes() {
        use crate::{
            Picker,
            event::MatchListEvent,
            match_list::{MatchList, Queued, SelectedIndices},
            render::StrRenderer,
        };

        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b"]);
        while picker.engine.update(5).matching {}
        picker
            .list_state
            .layout
            .update_items(picker.engine.snapshot(), &picker.list_state.config);
        let mut list = MatchList::new(
            &mut picker.list_state,
            &picker.chars,
            SelectedIndices::init(None),
        );
        let mut status = StatusLine::new(&picker.chars);
        assert!(status.update(picker.engine.update(5), list.queued().count(None), false));

        list.handle(MatchListEvent::Up(1), &picker.engine);
        list.flush(&picker.engine);
        let matching = picker.engine.update(5);
        assert!(list.update(&picker.engine, matching.items_changed, false));
        assert!(!status.update(matching, list.queued().count(None), false));

        list.handle(MatchListEvent::ToggleDown(0), &picker.engine);
        list.handle(MatchListEvent::ToggleDown(0), &picker.engine);
        list.flush(&picker.engine);
        let matching = picker.engine.update(5);
        assert!(list.update(&picker.engine, matching.items_changed, false));
        assert!(!status.update(matching, list.queued().count(None), false));
        assert_eq!(list.queued().len(), 0);

        list.handle(MatchListEvent::ToggleDown(0), &picker.engine);
        list.flush(&picker.engine);
        let matching = picker.engine.update(5);
        assert!(list.update(&picker.engine, matching.items_changed, false));
        assert!(status.update(matching, list.queued().count(None), false));
        assert_eq!(list.queued().len(), 1);

        list.handle(MatchListEvent::UnqueueAll, &picker.engine);
        list.handle(MatchListEvent::Down(1), &picker.engine);
        list.handle(MatchListEvent::ToggleDown(0), &picker.engine);
        list.flush(&picker.engine);
        let matching = picker.engine.update(5);
        assert!(list.update(&picker.engine, matching.items_changed, false));
        assert!(!status.update(matching, list.queued().count(None), false));
        assert_eq!(list.queued().len(), 1);
    }

    #[test]
    fn counts_follow_the_latest_matcher_update() {
        use crate::{Picker, render::StrRenderer};

        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b"]);
        while picker.engine.update(5).matching {}
        let mut status = StatusLine::new(&picker.chars);
        assert!(status.update(picker.engine.update(5), None, false));

        picker.engine.reparse("a");
        let matching = loop {
            let matching = picker.engine.update(5);
            if !matching.matching {
                break matching;
            }
        };
        assert!(status.update(matching, None, false));
        let values = status.previous.unwrap();
        assert_eq!((values.matched, values.total), (1, 2));
        assert!(!status.update(matching, None, false));
    }

    #[test]
    fn hidden_status_consumes_updates_and_resize_does_not_invalidate_again() {
        let chars = PickerChars::new();
        let mut status = StatusLine::new(&chars);
        let mut matching = MatchStatus {
            items_changed: false,
            matched: 0,
            total: 0,
            matching: false,
            injecting: false,
        };
        assert!(status.update(matching, None, false));
        assert!(!status.update(matching, None, false));

        matching.injecting = true;
        assert!(!status.update(matching, None, false));
        let redraw = status.update(matching, None, true);
        assert!(redraw);
        assert!(!status.update(matching, None, false));

        status.resize(
            Area {
                width: 20,
                height: 1,
                ..Area::default()
            },
            &(),
        );
        let mut output = Vec::new();
        status
            .draw(
                &(),
                &mut CrosstermRect::new(
                    &mut output,
                    Area {
                        width: 20,
                        height: 1,
                        ..Area::default()
                    },
                    ClearState::Precleared,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(String::from_utf8(output).unwrap().contains("0/0"));
        assert!(!status.update(matching, None, false));
    }
}
