use crate::draw::{Color, Style};

use super::*;
use crate::{
    component::tests::TestSurface,
    preview::{BoundaryChars, Content},
};

fn pane(text: &str) -> Pane {
    let mut pane = Pane::new();
    pane.push_text(text);
    pane
}

#[test]
fn content_edits_preserve_view_state_and_pane_reset_discards_it() {
    let style = Style::new().fg(Color::Red);
    let mut buffer = Buffer::new();
    buffer.push_styled_text("first\nsecond", style);
    let mut pane = buffer
        .into_pane()
        .with_offset(usize::MAX, usize::MAX)
        .with_line_numbers(true);
    pane.set_err(true);
    assert!(
        pane.buffer()
            .line(1)
            .unwrap()
            .spans()
            .eq([style.apply("second")])
    );

    pane.buffer_mut().clear();
    pane.push_text("replacement");
    assert_eq!(pane.offset(), (usize::MAX, usize::MAX));
    assert!(pane.line_numbers());
    assert!(pane.is_err());

    let mut pane = pane.into_buffer().into_pane();
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "replacement");
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    pane.set_offset(9, 10);
    pane.set_line_numbers(true);
    pane.set_err(true);
    pane.clear();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    assert!(pane.buffer().lines().map(|line| line.as_str()).eq([""]));
}

#[test]
fn vertical_navigation_saturates_and_preserves_columns() {
    for (height, bottom) in [(0, 5), (1, 4), (3, 2), (5, 0), (8, 0)] {
        let mut pane = pane("a\nb\nc\nd\ne").with_offset(9, usize::MAX);
        let size = Dimensions { width: 20, height };
        pane.viewport(size).clamp_vertical();
        assert_eq!(pane.offset(), (9, bottom));
        pane.viewport(size).up(usize::MAX);
        assert_eq!(pane.offset(), (9, 0));
        pane.viewport(size).page_down(1);
        assert_eq!(pane.offset(), (9, usize::from(height).min(bottom)));
        pane.viewport(size).page_up(usize::MAX);
        assert_eq!(pane.offset(), (9, 0));
        pane.viewport(size).down(usize::MAX);
        assert_eq!(pane.offset(), (9, bottom));
        pane.viewport(size).align_top();
        assert_eq!(pane.offset(), (9, 0));
        pane.viewport(size).align_bottom();
        assert_eq!(pane.offset(), (9, bottom));
    }
    let mut pane = pane("a\nb\nc\nd\ne").with_offset(9, 4);
    for (height, row) in [(3, 2), (1, 2), (8, 0)] {
        pane.viewport(Dimensions { width: 20, height })
            .clamp_vertical();
        assert_eq!(pane.offset(), (9, row));
    }
}

#[test]
fn horizontal_bounds_use_visible_unicode_width_and_line_number_gutter() {
    let mut pane =
        pane("offscreen and much longer\ne\u{301}界👩🏽‍💻abc\n\nabcdef\noffscreen and much longer");
    for (row, width, height, numbers, right) in [
        (1, 5, 3, false, 3),
        (1, 5, 3, true, 5),
        (1, 3, 3, true, 5),
        (1, 1, 3, true, 7),
        (1, 0, 1, false, 8),
        (1, 5, 0, false, 0),
        (1, 12, 3, false, 0),
        (2, 5, 1, false, 0),
        (3, 5, 1, false, 1),
    ] {
        let size = Dimensions { width, height };
        pane.set_line_numbers(numbers);
        pane.set_offset(0, row);
        pane.viewport(size).right(usize::MAX);
        assert_eq!(pane.offset(), (right, row));
        pane.set_offset(usize::MAX, row);
        pane.viewport(size).align_right();
        assert_eq!(pane.offset(), (right, row));
        pane.viewport(size).align_left();
        assert_eq!(pane.offset(), (0, row));
    }
}

#[test]
fn scrolling_preserves_overshoot_until_explicitly_clamped() {
    let mut pane = pane("abcdefghijklmnop\nx\n\ne\u{301}界👩🏽‍💻abc");
    let size = Dimensions {
        width: 6,
        height: 1,
    };
    pane.set_offset(usize::MAX, usize::MAX);
    let _ = pane.viewport(size);
    assert_eq!(pane.offset(), (usize::MAX, usize::MAX));
    pane.viewport(size).clamp_vertical();
    assert_eq!(pane.offset(), (usize::MAX, 3));
    pane.viewport(size).right(10);
    assert_eq!(pane.offset(), (usize::MAX, 3));
    pane.viewport(size).clamp().left(1);
    assert_eq!(pane.offset(), (1, 3));

    pane.viewport(size).align_top().align_right().down(1);
    assert_eq!(pane.offset(), (10, 1));
    pane.viewport(size).right(1).clamp_vertical();
    assert_eq!(pane.offset(), (10, 1));
    pane.viewport(size).left(1).down(1);
    assert_eq!(pane.offset(), (9, 2));
    pane.viewport(size).clamp();
    assert_eq!(pane.offset(), (0, 2));

    pane.set_line_numbers(true);
    pane.set_offset(usize::MAX, usize::MAX);
    pane.viewport(size).clamp();
    assert_eq!(pane.offset(), (4, 3));
    pane.viewport(Dimensions::default()).align_bottom().clamp();
    assert_eq!(pane.offset(), (0, 4));
}

#[test]
fn grapheme_width_controls_navigation_and_matches_drawing() {
    let mut pane = pane("ＡＡＡＡＡ");
    let size = Dimensions {
        width: 3,
        height: 1,
    };
    pane.viewport(size).align_right();
    assert_eq!(pane.offset(), (7, 0));

    pane.set_offset(0, 0);
    pane.viewport(size).right(usize::MAX);
    assert_eq!(pane.offset(), (7, 0));

    let mut target = TestSurface::new(3, 1);
    crate::preview::draw(&mut target, Content::Ready(&pane), None, '…').unwrap();
    assert_eq!(target.rows[0], "…Ａ");
}

#[test]
fn bordered_preview_streams_rows_without_preclearing_or_interior_cursor_moves() {
    let pane = pane("hi\nx");
    let mut target = TestSurface::new(6, 4);
    crate::preview::draw(
        &mut target,
        Content::Ready(&pane),
        Some(BoundaryChars::ascii()),
        '…',
    )
    .unwrap();
    assert_eq!(target.rows, ["+----+", "|hi  |", "|x   |", "+----+"]);
    assert_eq!(target.moves, [(0, 0), (0, 3), (0, 1), (0, 2)]);
    assert_eq!(target.clear_calls, 0);
}

#[test]
fn empty_and_tiny_targets_keep_preview_borders_in_bounds() {
    let chars = BoundaryChars {
        top_left: 'a',
        top_right: 'b',
        bottom_left: 'c',
        bottom_right: 'd',
        horizontal: '-',
        vertical: '|',
    };
    let mut pane = pane("Ａabc\nsecond");
    pane.set_err(true);
    for width in 0..=4 {
        for height in 0..=4 {
            let mut target = TestSurface::new(width, height);
            crate::preview::draw(&mut target, Content::Ready(&pane), Some(chars), '…').unwrap();
            assert_eq!(target.touched, width != 0 && height != 0);
            if width == 0 || height == 0 {
                continue;
            }
            let top = if width == 1 {
                "a".to_owned()
            } else {
                format!("a{}b", "-".repeat(usize::from(width - 2)))
            };
            assert_eq!(target.rows[0], top);
            if height > 1 {
                let bottom = if width == 1 {
                    "c".to_owned()
                } else {
                    format!("c{}d", "-".repeat(usize::from(width - 2)))
                };
                assert_eq!(target.rows[usize::from(height - 1)], bottom);
            }
        }
    }
}

#[test]
fn empty_and_tiny_targets_keep_preview_content_in_bounds() {
    let pane = pane("Ａabc\nsecond").with_line_numbers(true);
    for width in 0..=4 {
        for height in 0..=4 {
            for content in [Content::Empty, Content::Loading, Content::Ready(&pane)] {
                let mut target = TestSurface::new(width, height);
                crate::preview::draw(&mut target, content, None, '…').unwrap();
                assert_eq!(target.touched, width != 0 && height != 0);
                if width == 0 || height == 0 {
                    continue;
                }
                for row in &target.rows {
                    assert_eq!(
                        unicode_width::UnicodeWidthStr::width(row.as_str()),
                        usize::from(width),
                    );
                }
            }
        }
    }
}
