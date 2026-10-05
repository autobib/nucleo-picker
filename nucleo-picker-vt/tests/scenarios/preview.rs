use std::{convert::Infallible, error::Error, time::Duration};

use crossterm::style::{ContentStyle, Stylize};
use nucleo_picker::{
    PickerOptions,
    event::{Event, MatchListEvent, PromptEvent},
    preview::{
        BoundaryChars, Preview, PreviewBuffer, PreviewEvent, PreviewRequest, PreviewResponse,
    },
};
use nucleo_picker_vt::{Driver, PaneSnapshot};
use unicode_width::UnicodeWidthStr;

use super::{ScenarioRunner, WAIT, lines};

struct ItemPreview;

impl Preview<String> for ItemPreview {
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        item: &String,
        request: PreviewRequest,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Infallible> {
        let mut buffer = request.ready();
        buffer.push_str(item);
        Ok(PreviewResponse::Ready(buffer))
    }
}

fn start(options: PickerOptions) -> ScenarioRunner {
    start_with("basic", lines(), options, ItemPreview)
}

fn start_with<P>(
    scenario_name: &'static str,
    items: Vec<&str>,
    options: PickerOptions,
    previewer: P,
) -> ScenarioRunner
where
    P: Preview<String, AbortErr = Infallible> + Send + 'static,
{
    crossterm::style::force_color_output(true);
    ScenarioRunner {
        scenario_name,
        driver: Driver::start_with_preview(items, options, previewer),
        timeout: WAIT,
        checkpoint_sequence: 0,
    }
}

fn assert_pane(snapshot: &PaneSnapshot, reversed: bool, contents: &[&str]) {
    assert_pane_with_border(snapshot, reversed, contents, BoundaryChars::new(), None);
}

fn assert_pane_with_border(
    snapshot: &PaneSnapshot,
    reversed: bool,
    contents: &[&str],
    chars: BoundaryChars,
    error_top: Option<&str>,
) {
    let width = usize::from(snapshot.size.cols / 2);
    let column = usize::from(snapshot.size.cols) - width;
    let height = usize::from(snapshot.size.rows);
    for (row, line) in snapshot.text.iter().enumerate() {
        let expected = if row == 0 {
            error_top.map_or_else(
                || {
                    format!(
                        "{}{}{}",
                        chars.top_left,
                        chars.horizontal.to_string().repeat(width - 2),
                        chars.top_right
                    )
                },
                str::to_owned,
            )
        } else if row == height - 1 {
            format!(
                "{}{}{}",
                chars.bottom_left,
                chars.horizontal.to_string().repeat(width - 2),
                chars.bottom_right
            )
        } else {
            let content = contents.get(row - 1).copied().unwrap_or_default();
            format!(
                "{}{content}{}{}",
                chars.vertical,
                " ".repeat(width - 2 - content.width()),
                chars.vertical
            )
        };
        let prefix = line
            .strip_suffix(&expected)
            .unwrap_or_else(|| panic!("unexpected row: {line:?}"));
        assert_eq!(prefix.width(), column);
    }
    assert_eq!(snapshot.text.len(), height);
    for row in 0..snapshot.size.rows {
        for col in column as u16..snapshot.size.cols {
            if row != 0
                && row != snapshot.size.rows - 1
                && col != column as u16
                && col != snapshot.size.cols - 1
            {
                continue;
            }
            let style = snapshot
                .styles
                .iter()
                .find(|span| span.row == row && span.start <= col && col < span.end);
            if error_top.is_some() {
                let style = &style.expect("missing error border style").style;
                assert_eq!(
                    style.foreground.as_ref().map(|color| color.0.as_str()),
                    Some("#cc6666")
                );
                assert_eq!(
                    style,
                    &nucleo_picker_vt::CellStyle {
                        foreground: style.foreground.clone(),
                        ..Default::default()
                    }
                );
            } else {
                assert!(style.is_none(), "unexpected border style: {style:?}");
            }
        }
    }
    assert!(
        snapshot
            .row_flags
            .iter()
            .all(|row| !row.wrapped && !row.continuation)
    );
    let cursor = snapshot.cursor.position.unwrap();
    assert!(usize::from(cursor.x) < column);
    assert_eq!(cursor.y, if reversed { 0 } else { snapshot.size.rows - 1 });
    assert!(!snapshot.cursor.pending_wrap);
}

#[test]
fn basic() -> Result<(), Box<dyn Error>> {
    let mut sr = start(PickerOptions::new());
    sr.wait_for_match_complete(24, 24)?;
    assert_pane(&sr.checkpoint("blank")?, false, &["item-00 alpha"]);
    checkpoint!(sr, "blank");
    sr.send(Event::Select)?;
    assert_eq!(sr.finish()?, ["item-00 alpha"]);
    Ok(())
}

#[test]
fn pane_survives_updates_and_resizes() -> Result<(), Box<dyn Error>> {
    for reversed in [false, true] {
        let mut sr = start(PickerOptions::new().reversed(reversed).highlight_line(true));
        sr.set_dimensions(23, 7)?;
        sr.wait_for_match_complete(24, 24)?;
        assert_pane(&sr.checkpoint("initial")?, reversed, &["item-00 …"]);

        sr.send(Event::Prompt(PromptEvent::Reset(
            "a long query containing 東京".to_owned(),
        )))?;
        sr.wait_for_match_complete(0, 24)?;
        assert_pane(&sr.checkpoint("no matches")?, reversed, &[]);
        sr.send(Event::Prompt(PromptEvent::Left(1)))?;
        assert_pane(&sr.checkpoint("prompt only")?, reversed, &[]);
        sr.send(Event::Redraw)?;
        assert_pane(&sr.checkpoint("full redraw")?, reversed, &[]);

        sr.send(Event::Prompt(PromptEvent::Reset(String::new())))?;
        sr.wait_for_match_complete(24, 24)?;
        sr.send(Event::MatchList(if reversed {
            MatchListEvent::Down(1)
        } else {
            MatchListEvent::Up(1)
        }))?;
        assert_pane(&sr.checkpoint("selection")?, reversed, &["item-01 …"]);

        for (width, height) in [(5, 7), (23, 2), (1, 1)] {
            sr.set_dimensions(width, height)?;
            let snapshot = sr.checkpoint("hidden")?;
            assert!(
                snapshot
                    .text
                    .iter()
                    .all(|row| !row.contains('╭') && !row.contains('│'))
            );
            assert!(snapshot.cursor.position.unwrap().x < width);
        }
        for (width, height) in [(6, 3), (61, 16)] {
            sr.set_dimensions(width, height)?;
            assert_pane(
                &sr.checkpoint("restored")?,
                reversed,
                &[if width == 6 { "…" } else { "item-01 bravo" }],
            );
        }

        sr.send(Event::Quit)?;
        assert!(sr.finish()?.is_empty());
    }
    Ok(())
}

struct TextPreview(fn(&str, &mut PreviewBuffer));

impl Preview<String> for TextPreview {
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        item: &String,
        request: PreviewRequest,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Infallible> {
        let mut buffer = request.ready();
        self.0(item, &mut buffer);
        Ok(PreviewResponse::Ready(buffer))
    }
}

#[test]
fn unicode_clipping_and_styles() -> Result<(), Box<dyn Error>> {
    let mut sr = start_with(
        "unicode_clipping_and_styles",
        vec!["item"],
        PickerOptions::new(),
        TextPreview(|_, buffer| {
            buffer.push_line("12345");
            buffer.push_styled_line("abcdef", ContentStyle::new().green().bold());
            buffer.push_line("界界界");
            buffer.push_str("e");
            buffer.push_styled_str("\u{301}", ContentStyle::new().blue());
            buffer.push_line("xy");
            buffer.push_str("👩");
            buffer.push_styled_str("🏽‍💻", ContentStyle::new().red());
            buffer.push_line("XYZ");
            buffer.push_str("ab界Z");
        }),
    );
    sr.wait_for_match_complete(1, 1)?;
    for (width, expected) in [
        (
            14,
            ["12345", "abcd…", "界界…", "e\u{301}xy", "👩🏽‍💻XYZ", "ab界Z"],
        ),
        (12, ["123…", "abc…", "界……", "e\u{301}xy", "👩🏽‍💻X…", "ab……"]),
        (10, ["12…", "ab…", "界…", "e\u{301}xy", "👩🏽‍💻…", "ab…"]),
        (8, ["1…", "a…", "……", "e\u{301}…", "……", "a…"]),
        (6, ["…", "…", "…", "…", "…", "…"]),
    ] {
        sr.set_dimensions(width, 9)?;
        let snapshot = sr.checkpoint(format!("width-{width}"))?;
        assert_pane(&snapshot, false, &expected);
        let column = width - width / 2 + 1;
        assert!(snapshot.styles.iter().all(|span| {
            span.end <= width / 2
                || (span.row == 2 && span.end < width - 1)
                || span.row == 4
                || span.row == 5
        }));
        if width == 14 {
            let styled = snapshot
                .styles
                .iter()
                .find(|span| span.row == 2 && span.start == column)
                .unwrap();
            assert!(styled.style.bold);
            assert!(styled.style.foreground.is_some());
            assert_eq!(styled.end, column + 4);
            checkpoint!(sr, "styled-unicode");
        }
    }
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

#[test]
fn preview_and_matches_use_the_same_grapheme_widths() -> Result<(), Box<dyn Error>> {
    let mut sr = start_with(
        "preview_and_matches_use_the_same_grapheme_widths",
        vec!["لاX"],
        PickerOptions::new().highlight_line(true),
        TextPreview(|_, buffer| {
            buffer.push_line("لا");
            buffer.push_line("لاX");
        }),
    );
    sr.type_text("لا")?;
    sr.wait_for_match_complete(1, 1)?;
    for (width, first, second, selected) in [
        (12, "│لا  │", "│لاX │", "▌ لاX "),
        (10, "│لا │", "│لاX│", "▌ لاX"),
        (8, "│لا│", "│ل…│", "▌ ل…"),
        (6, "│…│", "│…│", "▌ …"),
    ] {
        sr.set_dimensions(width, 5)?;
        let snapshot = sr.checkpoint(format!("width-{width}"))?;
        assert!(snapshot.text[1].ends_with(first), "{:?}", snapshot.text);
        assert!(snapshot.text[2].ends_with(second), "{:?}", snapshot.text);
        assert!(
            snapshot.text[2].starts_with(selected),
            "{:?}",
            snapshot.text
        );
        assert!(snapshot.row_flags.iter().all(|row| !row.wrapped));
    }
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

#[test]
fn style_resets_and_custom_elision() -> Result<(), Box<dyn Error>> {
    let mut sr = start_with(
        "style_resets_and_custom_elision",
        vec!["item"],
        PickerOptions::new().truncation_ellipsis('~'),
        TextPreview(|_, buffer| {
            buffer.push_styled_str("AB", ContentStyle::new().green().on_blue().bold());
            buffer.push_str("C");
            buffer.push_styled_str(
                "D",
                ContentStyle::new().underline(crossterm::style::Color::Red),
            );
            buffer.push_styled_str("E", ContentStyle::new().underlined());
            buffer.push_line("F");
            buffer.push_str("abcdef");
        }),
    );
    sr.set_dimensions(16, 6)?;
    sr.wait_for_match_complete(1, 1)?;
    let snapshot = sr.checkpoint("styles")?;
    assert_pane(&snapshot, false, &["ABCDEF", "abcdef"]);
    let spans: Vec<_> = snapshot
        .styles
        .iter()
        .filter(|span| span.start > 8)
        .collect();
    assert_eq!(spans.len(), 3);
    assert_eq!((spans[0].start, spans[0].end), (9, 11));
    assert!(spans[0].style.bold);
    assert!(spans[0].style.background.is_some());
    assert_eq!((spans[1].start, spans[1].end), (12, 13));
    assert!(spans[1].style.underline_color.is_some());
    assert_eq!((spans[2].start, spans[2].end), (13, 14));
    assert!(spans[2].style.underline_color.is_none());
    assert_eq!(
        spans[2].style.underline,
        nucleo_picker_vt::UnderlineName::Single
    );
    checkpoint!(sr, "styles");
    sr.set_dimensions(10, 6)?;
    assert_pane(&sr.checkpoint("custom marker")?, false, &["AB~", "ab~"]);
    checkpoint!(sr, "custom-marker");
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());

    let mut sr = start_with(
        "ascii_elision",
        vec!["abcdef"],
        PickerOptions::new().ascii_compatible(true),
        ItemPreview,
    );
    sr.set_dimensions(10, 5)?;
    sr.wait_for_match_complete(1, 1)?;
    let snapshot = sr.checkpoint("ascii marker")?;
    assert!(snapshot.text.iter().all(|line| line.is_ascii()));
    for (line, expected) in snapshot
        .text
        .iter()
        .zip(["+---+", "|ab.|", "|   |", "|   |", "+---+"])
    {
        assert!(line.ends_with(expected), "unexpected row: {line:?}");
    }
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

#[test]
fn boundary_chars() -> Result<(), Box<dyn Error>> {
    const CUSTOM: BoundaryChars = BoundaryChars {
        top_left: '1',
        top_right: '2',
        bottom_left: '3',
        bottom_right: '4',
        vertical: '!',
        horizontal: '=',
    };
    const CUSTOM_OPTIONS: PickerOptions = PickerOptions::new().preview_boundary_chars(CUSTOM);
    let unicode = ["╭───╮", "│ab…│", "│   │", "│   │", "╰───╯"];
    for (options, expected) in [
        (
            CUSTOM_OPTIONS,
            ["1===2", "!ab…!", "!   !", "!   !", "3===4"],
        ),
        (
            PickerOptions::new()
                .ascii_compatible(true)
                .preview_boundary_chars(CUSTOM),
            ["1===2", "!ab.!", "!   !", "!   !", "3===4"],
        ),
        (
            CUSTOM_OPTIONS.ascii_compatible(true),
            ["+---+", "|ab.|", "|   |", "|   |", "+---+"],
        ),
        (CUSTOM_OPTIONS.ascii_compatible(false), unicode),
        (
            PickerOptions::new()
                .ascii_compatible(true)
                .ascii_compatible(false),
            unicode,
        ),
        (
            PickerOptions::new()
                .ascii_compatible(true)
                .preview_boundary_chars(BoundaryChars::default()),
            ["╭───╮", "│ab.│", "│   │", "│   │", "╰───╯"],
        ),
        (
            PickerOptions::new().preview_boundary_chars(BoundaryChars::ascii()),
            ["+---+", "|ab…|", "|   |", "|   |", "+---+"],
        ),
    ] {
        let mut sr = start_with("boundary_chars", vec!["abcdef"], options, ItemPreview);
        sr.set_dimensions(10, 5)?;
        sr.wait_for_match_complete(1, 1)?;
        let snapshot = sr.checkpoint("boundary")?;
        assert_eq!(snapshot.text.len(), expected.len());
        for (line, expected) in snapshot.text.iter().zip(expected) {
            let prefix = line
                .strip_suffix(expected)
                .unwrap_or_else(|| panic!("unexpected row: {line:?}"));
            assert_eq!(prefix.width(), 5);
        }
        sr.send(Event::Quit)?;
        assert!(sr.finish()?.is_empty());
    }
    Ok(())
}

#[test]
fn numbered_scrolling_and_blank_rows() -> Result<(), Box<dyn Error>> {
    let assert_numbers = |snapshot: &PaneSnapshot, expected: &[(u16, u16, u16)]| {
        let column = snapshot.size.cols - snapshot.size.cols / 2;
        let spans: Vec<_> = snapshot
            .styles
            .iter()
            .filter(|span| span.start > column)
            .collect();
        assert_eq!(spans.len(), expected.len());
        for (span, &(row, start, end)) in spans.into_iter().zip(expected) {
            assert_eq!((span.row, span.start, span.end), (row, start, end));
            assert_eq!(
                span.style,
                nucleo_picker_vt::CellStyle {
                    background: Some(nucleo_picker_vt::HexColor("#1d1f21".to_owned())),
                    ..Default::default()
                }
            );
        }
    };
    let mut sr = start_with(
        "numbered_scrolling_and_blank_rows",
        vec!["long", "short", "empty"],
        PickerOptions::new().preview_line_numbers(true),
        TextPreview(|item, buffer| match item {
            "long" => {
                for _ in 1..100 {
                    buffer.push_line("text");
                }
                buffer.push_str("text");
            }
            "short" => buffer.push_text("short\n\nlast"),
            _ => {}
        }),
    );
    sr.set_dimensions(26, 6)?;
    sr.wait_for_match_complete(3, 3)?;
    let initial = sr.checkpoint("initial")?;
    assert_pane(
        &initial,
        false,
        &["  1 text", "  2 text", "  3 text", "  4 text"],
    );
    assert_numbers(
        &initial,
        &[(1, 14, 17), (2, 14, 17), (3, 14, 17), (4, 14, 17)],
    );
    checkpoint!(sr, "initial");
    for (event, expected) in [
        (
            PreviewEvent::Down(7),
            ["  8 text", "  9 text", " 10 text", " 11 text"],
        ),
        (
            PreviewEvent::PageDown(1),
            [" 12 text", " 13 text", " 14 text", " 15 text"],
        ),
        (
            PreviewEvent::PageUp(1),
            ["  8 text", "  9 text", " 10 text", " 11 text"],
        ),
        (
            PreviewEvent::Down(usize::MAX),
            [" 97 text", " 98 text", " 99 text", "100 text"],
        ),
    ] {
        let bottom = matches!(event, PreviewEvent::Down(usize::MAX));
        sr.send(Event::Preview(event))?;
        let snapshot = sr.checkpoint("scroll")?;
        assert_pane(&snapshot, false, &expected);
        if bottom {
            assert_numbers(
                &snapshot,
                &[(1, 14, 17), (2, 14, 17), (3, 14, 17), (4, 14, 17)],
            );
        }
    }
    checkpoint!(sr, "bottom");
    sr.set_dimensions(12, 6)?;
    let narrow = sr.checkpoint("hidden numbers")?;
    assert_pane(&narrow, false, &["text", "text", "text", "text"]);
    assert!(narrow.styles.iter().all(|span| span.end <= 6));
    sr.set_dimensions(14, 6)?;
    assert_pane(
        &sr.checkpoint("one content column")?,
        false,
        &[" 97 …", " 98 …", " 99 …", "100 …"],
    );
    sr.set_dimensions(26, 8)?;
    assert_pane(
        &sr.checkpoint("resize at bottom")?,
        false,
        &[
            " 95 text", " 96 text", " 97 text", " 98 text", " 99 text", "100 text",
        ],
    );
    sr.send(Event::MatchList(MatchListEvent::Up(1)))?;
    let short = sr.checkpoint("short")?;
    assert_pane(&short, false, &["1 short", "2 ", "3 last"]);
    assert_numbers(&short, &[(1, 14, 15), (2, 14, 15), (3, 14, 15)]);
    checkpoint!(sr, "short");
    sr.send(Event::MatchList(MatchListEvent::Up(1)))?;
    assert_pane(&sr.checkpoint("empty")?, false, &["1 "]);
    sr.send(Event::MatchList(MatchListEvent::Down(2)))?;
    assert_pane(
        &sr.checkpoint("cached scroll")?,
        false,
        &[
            " 95 text", " 96 text", " 97 text", " 98 text", " 99 text", "100 text",
        ],
    );
    sr.send(Event::Prompt(PromptEvent::Reset("no matches".to_owned())))?;
    sr.wait_for_match_complete(0, 3)?;
    let empty = sr.checkpoint("no matches")?;
    assert_pane(&empty, false, &[]);
    assert!(empty.styles.iter().all(|span| span.end <= 13));
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

struct DeferredPreview(std::sync::mpsc::Sender<nucleo_picker::preview::QueuedPreviewRequest>);
impl Preview<String> for DeferredPreview {
    type AbortErr = Infallible;

    fn preview(
        &mut self,
        item: &String,
        request: PreviewRequest,
        _timeout: Duration,
    ) -> Result<PreviewResponse, Infallible> {
        if item == "ready" {
            let mut buffer = request.ready();
            buffer.push_styled_text("first\nsecond\nthird", ContentStyle::new().on_blue());
            Ok(PreviewResponse::Ready(buffer))
        } else {
            let (pending, queued) = request.defer();
            self.0.send(queued).unwrap();
            Ok(PreviewResponse::Pending(pending))
        }
    }
}

#[test]
fn pending_previews_clear_stale_content_and_render_on_publication() -> Result<(), Box<dyn Error>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut sr = start_with(
        "pending_previews",
        vec!["ready", "deferred"],
        PickerOptions::new().preview_line_numbers(true),
        DeferredPreview(sender),
    );
    sr.set_dimensions(30, 6)?;
    sr.wait_for_match_complete(2, 2)?;
    assert_pane(
        &sr.checkpoint("ready")?,
        false,
        &["1 first", "2 second", "3 third"],
    );
    sr.send(Event::MatchList(MatchListEvent::Up(1)))?;
    let pending = sr.checkpoint("pending")?;
    assert_pane(&pending, false, &["Loading..."]);
    let styles: Vec<_> = pending
        .styles
        .iter()
        .filter(|span| span.start >= 15)
        .collect();
    assert_eq!(styles.len(), 1);
    assert_eq!((styles[0].row, styles[0].start, styles[0].end), (1, 16, 26));
    assert!(styles[0].style.foreground.is_some());
    assert!(styles[0].style.background.is_none());
    for (width, expected) in [(24, "Loading..."), (22, "Loading.…"), (6, "…")] {
        sr.set_dimensions(width, 3)?;
        assert_pane(&sr.checkpoint("narrow pending")?, false, &[expected]);
    }
    sr.set_dimensions(30, 6)?;
    checkpoint!(sr, "pending");
    let active = receiver.recv_timeout(WAIT)?.start().unwrap();
    let mut buffer = PreviewBuffer::new();
    buffer.push_styled_str("failed", ContentStyle::new().red());
    buffer.set_err(true);
    assert!(active.publish(&mut buffer));
    sr.wait_for(|status| status.changed)?;
    assert_pane_with_border(
        &sr.checkpoint("published")?,
        false,
        &["1 failed"],
        BoundaryChars::new(),
        Some("╭─ Error ─────╮"),
    );
    checkpoint!(sr, "error");
    sr.send(Event::MatchList(MatchListEvent::Down(1)))?;
    assert_pane(
        &sr.checkpoint("ready again")?,
        false,
        &["1 first", "2 second", "3 third"],
    );
    sr.send(Event::MatchList(MatchListEvent::Up(1)))?;
    assert_pane_with_border(
        &sr.checkpoint("cached error")?,
        false,
        &["1 failed"],
        BoundaryChars::new(),
        Some("╭─ Error ─────╮"),
    );
    sr.send(Event::Prompt(PromptEvent::Reset("no matches".to_owned())))?;
    sr.wait_for_match_complete(0, 2)?;
    let empty = sr.checkpoint("no matches")?;
    assert_pane(&empty, false, &[]);
    assert!(empty.styles.iter().all(|span| span.end <= 15));
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

#[test]
fn error_labels_fit_and_respect_boundary_chars() -> Result<(), Box<dyn Error>> {
    for chars in [
        BoundaryChars::new(),
        BoundaryChars::ascii(),
        BoundaryChars {
            top_left: '1',
            top_right: '2',
            bottom_left: '3',
            bottom_right: '4',
            vertical: '!',
            horizontal: '=',
        },
    ] {
        let mut sr = start_with(
            "error_labels",
            vec!["error"],
            PickerOptions::new().preview_boundary_chars(chars),
            TextPreview(|_, buffer| buffer.set_err(true)),
        );
        sr.wait_for_match_complete(1, 1)?;
        for top in [
            "╭─╮",
            "╭──╮",
            "╭───╮",
            "╭────╮",
            "╭ Err ╮",
            "╭ Err ─╮",
            "╭ Error ╮",
            "╭ Error ─╮",
            "╭─ Error ─╮",
            "╭─ Error ──╮",
            "╭─ Error ───╮",
        ] {
            let width = top.width() as u16;
            let top: String = top
                .chars()
                .map(|ch| match ch {
                    '╭' => chars.top_left,
                    '╮' => chars.top_right,
                    '─' => chars.horizontal,
                    _ => ch,
                })
                .collect();
            sr.set_dimensions(width * 2, 3)?;
            let snapshot = sr.checkpoint("label width")?;
            assert_pane_with_border(&snapshot, false, &[], chars, Some(&top));
            assert!(
                snapshot
                    .styles
                    .iter()
                    .filter(|span| span.row == 1 && span.start >= width)
                    .all(|span| span.end == width + 1 || span.start == width * 2 - 1)
            );
            if chars == BoundaryChars::new() && width >= 6 {
                checkpoint!(sr, format!("width-{width}"));
            }
        }
        sr.send(Event::Quit)?;
        assert!(sr.finish()?.is_empty());
    }
    Ok(())
}

#[test]
fn error_previews_preserve_contents_and_scrolling() -> Result<(), Box<dyn Error>> {
    let mut sr = start_with(
        "error_contents",
        vec!["error", "ready"],
        PickerOptions::new().preview_line_numbers(true),
        TextPreview(|item, buffer| {
            buffer.set_err(item == "error");
            buffer.push_styled_str("AB", ContentStyle::new().green().on_blue().bold());
            buffer.push_line("C");
            buffer.push_text("second\nthird\nfourth\nfifth");
        }),
    );
    sr.set_dimensions(30, 5)?;
    sr.wait_for_match_complete(2, 2)?;
    let error = sr.checkpoint("error")?;
    assert_pane_with_border(
        &error,
        false,
        &["1 ABC", "2 second", "3 third"],
        BoundaryChars::new(),
        Some("╭─ Error ─────╮"),
    );
    sr.send(Event::Preview(PreviewEvent::Down(usize::MAX)))?;
    assert_pane_with_border(
        &sr.checkpoint("scrolled error")?,
        false,
        &["3 third", "4 fourth", "5 fifth"],
        BoundaryChars::new(),
        Some("╭─ Error ─────╮"),
    );
    sr.send(Event::MatchList(MatchListEvent::Up(1)))?;
    let ready = sr.checkpoint("ready")?;
    assert_pane(&ready, false, &["1 ABC", "2 second", "3 third"]);
    let contents = |snapshot: &PaneSnapshot| {
        snapshot
            .styles
            .iter()
            .filter(|span| span.start > 15 && span.end < 30)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(contents(&error), contents(&ready));
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}

#[test]
fn pending_previews_distinguish_no_selection_and_publish_success() -> Result<(), Box<dyn Error>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut sr = start_with(
        "pending_success",
        vec!["deferred"],
        PickerOptions::new()
            .ascii_compatible(true)
            .truncation_ellipsis('~'),
        DeferredPreview(sender),
    );
    sr.set_dimensions(10, 3)?;
    sr.wait_for_match_complete(1, 1)?;
    assert_pane_with_border(
        &sr.checkpoint("narrow pending")?,
        false,
        &["Lo~"],
        BoundaryChars::ascii(),
        None,
    );
    let active = receiver.recv_timeout(WAIT)?.start().unwrap();
    sr.set_dimensions(30, 5)?;
    sr.send(Event::Prompt(PromptEvent::Reset("no matches".to_owned())))?;
    sr.wait_for_match_complete(0, 1)?;
    let empty = sr.checkpoint("no matches")?;
    assert_pane_with_border(&empty, false, &[], BoundaryChars::ascii(), None);
    assert!(empty.styles.iter().all(|span| span.end <= 15));
    sr.send(Event::Prompt(PromptEvent::Reset(String::new())))?;
    sr.wait_for_match_complete(1, 1)?;
    assert_pane_with_border(
        &sr.checkpoint("pending again")?,
        false,
        &["Loading..."],
        BoundaryChars::ascii(),
        None,
    );
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("complete");
    assert!(active.publish(&mut buffer));
    sr.wait_for(|status| status.changed)?;
    let ready = sr.checkpoint("published")?;
    assert_pane_with_border(&ready, false, &["complete"], BoundaryChars::ascii(), None);
    assert!(ready.styles.iter().all(|span| span.end <= 15));
    sr.send(Event::Quit)?;
    assert!(sr.finish()?.is_empty());
    Ok(())
}
