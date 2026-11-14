use super::*;
use crossterm::style::{ContentStyle, Stylize};

#[test]
fn conversion_and_content_edits_preserve_storage_and_separate_view_state() {
    let style = ContentStyle::new().red();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_text("first\nsecond", style);
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    let mut pane = buffer.into_pane();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    pane = pane
        .with_offset(usize::MAX, usize::MAX)
        .with_line_numbers(true);
    pane.set_err(true);
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
    assert!(
        pane.buffer()
            .line(1)
            .unwrap()
            .spans()
            .eq([style.apply("second")])
    );
    assert!(pane.is_err());

    pane.buffer_mut().clear();
    assert_eq!(pane.offset(), (usize::MAX, usize::MAX));
    assert!(pane.line_numbers());
    assert!(pane.is_err());
    pane.push_text("reused");
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
    assert!(pane.is_err());
    let mut pane = pane.into_buffer().into_pane();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "reused");
    pane.set_offset(9, 10);
    pane.set_line_numbers(true);
    pane.set_err(true);
    pane.clear();
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    assert_eq!(pane.buffer().lines().len(), 1);
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
}

fn ready(lines: usize) -> PreviewPane {
    let mut buffer = PreviewBuffer::new();
    for _ in 1..lines {
        buffer.newline();
    }
    buffer.into_pane()
}

fn text_preview(text: &str) -> PreviewPane {
    let mut buffer = ready(1);
    buffer.buffer_mut().push_text(text);
    buffer
}

#[test]
fn vertical_alignment_and_clamping_preserve_columns() {
    for height in [1, 8, 23, 40] {
        let mut buffer = ready(23);
        buffer.set_offset(9, 3);
        let size = PreviewSize { width: 20, height };
        let bottom = 23_usize.saturating_sub(usize::from(height));
        for _ in 0..2 {
            buffer.viewport(size).align_bottom();
            assert_eq!(buffer.offset(), (9, bottom));
        }
        for _ in 0..2 {
            buffer.viewport(size).align_top();
            assert_eq!(buffer.offset(), (9, 0));
        }
    }
    let mut buffer = ready(23);
    buffer.set_offset(9, usize::MAX);
    for (height, row) in [(8, 15), (12, 11), (4, 11), (30, 0)] {
        buffer
            .viewport(PreviewSize { width: 20, height })
            .clamp_vertical();
        assert_eq!(buffer.offset(), (9, row));
    }
}

#[test]
fn viewport_chaining_and_clamping_use_current_rows() {
    let mut pane = text_preview("abcdefghijklmnop\nx\n\ne\u{301}界👩🏽‍💻abc");
    let size = PreviewSize {
        width: 6,
        height: 1,
    };
    pane.set_offset(usize::MAX, usize::MAX);
    {
        let _viewport = pane.viewport(size);
    }
    assert_eq!(pane.offset(), (usize::MAX, usize::MAX));

    pane.viewport(size).clamp_vertical();
    assert_eq!(pane.offset(), (usize::MAX, 3));
    pane.viewport(size).clamp();
    assert_eq!(pane.offset(), (2, 3));
    pane.viewport(size).left(1).clamp();
    assert_eq!(pane.offset(), (1, 3));

    pane.viewport(size).align_top().align_right().down(1);
    assert_eq!(pane.offset(), (10, 1));
    pane.viewport(size).clamp_vertical();
    assert_eq!(pane.offset(), (10, 1));
    pane.viewport(size).clamp();
    assert_eq!(pane.offset(), (0, 1));
    pane.viewport(size).align_bottom().clamp();
    assert_eq!(pane.offset(), (0, 3));

    pane.set_line_numbers(true);
    pane.set_offset(usize::MAX, usize::MAX);
    pane.viewport(size).clamp();
    assert_eq!(pane.offset(), (4, 3));
    pane.viewport(size).align_top().down(2).align_right();
    assert_eq!(pane.offset(), (0, 2));
}

#[test]
fn horizontal_bounds_and_alignment_use_visible_columns_and_line_numbers() {
    let mut buffer = text_preview(
        "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\n\nabcdef\noffscreen and much longer",
    );
    let size = PreviewSize {
        width: 5,
        height: 3,
    };
    buffer.set_offset(0, 1);
    buffer.viewport(size).left(1);
    assert_eq!(buffer.offset(), (0, 1));
    buffer.viewport(size).right(1);
    assert_eq!(buffer.offset(), (1, 1));
    for columns in [usize::MAX, 0, 1, usize::MAX] {
        buffer.viewport(size).right(columns);
        assert_eq!(buffer.offset(), (3, 1));
    }
    for (columns, expected) in [(1, 2), (usize::MAX, 0), (0, 0)] {
        buffer.viewport(size).left(columns);
        assert_eq!(buffer.offset(), (expected, 1));
    }
    for _ in 0..2 {
        buffer.viewport(size).align_right();
        assert_eq!(buffer.offset(), (3, 1));
    }
    for _ in 0..2 {
        buffer.viewport(size).align_left();
        assert_eq!(buffer.offset(), (0, 1));
    }
    for column in [usize::MAX - 1, usize::MAX] {
        buffer.set_offset(column, 1);
        buffer.viewport(size).right(10);
        assert_eq!(buffer.offset(), (column, 1));
        buffer.viewport(size).align_right();
        assert_eq!(buffer.offset(), (3, 1));
    }
    buffer.set_line_numbers(true);
    assert_eq!(buffer.number_width(6), 2);
    buffer.viewport(size).right(usize::MAX);
    assert_eq!(buffer.offset(), (5, 1));
    buffer.viewport(size).align_right();
    assert_eq!(buffer.offset(), (5, 1));
    buffer.set_line_numbers(false);
    buffer.viewport(size).right(1);
    assert_eq!(buffer.offset(), (5, 1));
    buffer.viewport(size).align_right();
    assert_eq!(buffer.offset(), (3, 1));
    buffer
        .viewport(PreviewSize { width: 12, ..size })
        .align_right();
    assert_eq!(buffer.offset(), (0, 1));
    buffer.viewport(size).align_right();
    assert_eq!(buffer.offset(), (3, 1));
    buffer.viewport(PreviewSize { height: 1, ..size }).down(1);
    assert_eq!(buffer.offset(), (3, 2));
    buffer
        .viewport(PreviewSize { height: 1, ..size })
        .align_right();
    assert_eq!(buffer.offset(), (0, 2));

    buffer.set_line_numbers(true);
    for width in [0, 1, 2, 3] {
        assert_eq!(buffer.number_width(width), 0);
    }
    buffer.set_offset(0, 1);
    buffer
        .viewport(PreviewSize { width: 1, ..size })
        .right(usize::MAX);
    assert_eq!(buffer.offset(), (7, 1));
    for text in ["", "abc", "abcde"] {
        let mut buffer = text_preview(text);
        let size = PreviewSize { height: 1, ..size };
        buffer.viewport(size).right(usize::MAX);
        assert_eq!(buffer.offset(), (0, 0));
        buffer.viewport(size).align_right();
        assert_eq!(buffer.offset(), (0, 0));
        buffer.viewport(size).align_left();
        assert_eq!(buffer.offset(), (0, 0));
    }
}

#[test]
fn zero_sized_geometry_handles_scrolling_and_alignment() {
    let mut buffer = text_preview("abcdefghijklmnop\nx\n\ny");
    for (width, height, right, aligned) in [(0, 1, 8, 16), (1, 0, 7, 0), (0, 0, 7, 0)] {
        buffer.set_offset(7, 0);
        let size = PreviewSize { width, height };
        buffer.viewport(size).right(1);
        assert_eq!(buffer.offset(), (right, 0));
        buffer.viewport(size).align_right();
        assert_eq!(buffer.offset(), (aligned, 0));
    }
    let size = PreviewSize::default();
    buffer.set_offset(7, 2);
    buffer.viewport(size).clamp_vertical();
    assert_eq!(buffer.offset(), (7, 2));
    buffer.viewport(size).up(1);
    assert_eq!(buffer.offset(), (7, 1));
    buffer.viewport(size).down(1);
    assert_eq!(buffer.offset(), (7, 2));
    buffer.viewport(size).align_bottom();
    assert_eq!(buffer.offset(), (7, 4));
    buffer.set_offset(7, usize::MAX);
    buffer.viewport(size).clamp_vertical();
    assert_eq!(buffer.offset(), (7, 4));
    buffer.viewport(size).clamp();
    assert_eq!(buffer.offset(), (0, 4));
}

#[test]
fn vertical_scroll_and_clamping_preserve_horizontal_position() {
    let mut buffer = text_preview("abcdefghijklmnop\nx\n\nabcdefghijklmnop");
    let size = PreviewSize {
        width: 6,
        height: 1,
    };
    buffer.viewport(size).right(usize::MAX);
    assert_eq!(buffer.offset(), (10, 0));
    for row in 1..=2 {
        buffer.viewport(size).down(1);
        assert_eq!(buffer.offset(), (10, row));
        buffer.viewport(size).right(1);
        assert_eq!(buffer.offset(), (10, row));
    }
    buffer.viewport(size).left(1);
    assert_eq!(buffer.offset(), (9, 2));
    buffer.viewport(size).up(usize::MAX);
    assert_eq!(buffer.offset(), (9, 0));
    let size = PreviewSize { height: 3, ..size };
    buffer.viewport(size).clamp_vertical();
    assert_eq!(buffer.offset(), (9, 0));
    buffer.viewport(PreviewSize { width: 40, ..size }).right(1);
    assert_eq!(buffer.offset(), (9, 0));
    buffer.viewport(PreviewSize { width: 5, ..size }).right(1);
    assert_eq!(buffer.offset(), (10, 0));
}

#[test]
fn horizontal_measurement_skips_offscreen_rows_and_hidden_suffixes() {
    use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};

    let mut buffer = PreviewBuffer::new();
    let long = "界".repeat(1_000_000);
    buffer.push_line(&long);
    buffer.push_line("abc");
    buffer.push_line("defghij");
    buffer.push_line(&long);
    buffer.push_str(&long);
    let mut pane = buffer.into_pane();

    pane.set_offset(0, 1);
    let viewport = pane.viewport(PreviewSize {
        width: 5,
        height: 2,
    });
    MEASURED_BYTES.set(0);
    assert_eq!(
        viewport.horizontal_limit::<CountingProcessor>(usize::MAX),
        2
    );
    assert_eq!(MEASURED_BYTES.get(), 10);

    pane.set_offset(0, 3);
    let viewport = pane.viewport(PreviewSize {
        width: 20,
        height: 2,
    });
    MEASURED_BYTES.set(0);
    assert_eq!(viewport.horizontal_limit::<CountingProcessor>(1), 1);
    assert_eq!(MEASURED_BYTES.get(), 33);

    pane.set_offset(0, 1);
    let viewport = pane.viewport(PreviewSize {
        width: 20,
        height: 4,
    });
    MEASURED_BYTES.set(0);
    assert_eq!(viewport.horizontal_limit::<CountingProcessor>(1), 1);
    assert_eq!(MEASURED_BYTES.get(), 43);
}
