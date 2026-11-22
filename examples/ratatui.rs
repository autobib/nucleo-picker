//! # Ratatui Demo
//!
//! This is an example showing how to use the session API to integrate the picker into a larger
//! application using Ratatui.
//!
//! Run with
//! ```sh
//! cargo run --example ratatui --features ratatui --release
//! ```

use std::{
    convert::Infallible,
    io,
    mem::DropGuard,
    time::{Duration, Instant},
};

use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event as TerminalEvent, KeyCode,
        KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use nucleo_picker::{
    event::{Event, LayoutEvent, PromptEvent, keybind_single_preview},
    interface::UncachedPreview,
    preview::Pane,
    render::StrRenderer,
    session::{DriverOptions, Exit, FrameOptions},
};
use ratatui_core::{
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    terminal::Terminal,
    text::{Line, Text},
};
use ratatui_crossterm::CrosstermBackend;
use ratatui_widgets::{block::Block, borders::BorderType};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let restore = DropGuard::new((), |()| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
    });
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let selected = run(&mut terminal)?;
    drop(terminal);
    drop(restore);
    if let Some(selected) = selected {
        println!("Selected {selected}");
    }
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<Option<&'static str>> {
    // initialize the underlying match engine driver
    let mut driver = DriverOptions::new().list_reversed(true).driver(StrRenderer);

    // initialize the preview interface
    let interface = UncachedPreview::new_single(|item: &&'static str, pane: &mut Pane| {
        pane.push_text("Selected item:\n\n");
        pane.push_text(item);
    });
    driver.push_batch(["red", "green", "blue", "yellow", "purple"]);

    // initialize a new session.
    let mut session = driver.session(interface);
    let mut focused = true;
    let mut next_background = Instant::now();
    let title = Line::from("nucleo-picker Ratatui demo").bold().centered();
    let underline = Line::from("─".repeat(title.width())).dark_gray().centered();

    let exit = loop {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(16);
        let background = now >= next_background;
        if background {
            next_background = now + Duration::from_millis(100);
        }
        session.update_matches(Duration::from_millis(1));
        let mut prepared =
            session.prepare_frame(FrameOptions::new(deadline).background(background));
        terminal.draw(|frame| {
            let [title_area, underline_area, _, content, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(frame.area());
            frame.render_widget(&title, title_area);
            frame.render_widget(&underline, underline_area);
            frame.render_widget(
                Line::from("Tab: focus | Enter: select | Alt-P: toggle preview | Esc: quit"),
                footer,
            );
            let visible = prepared.visible_components();
            let [main, right] =
                Layout::horizontal([Constraint::Min(0), Constraint::Percentage(40)]).areas(content);
            let [preview, application] = Layout::vertical([
                Constraint::Percentage(if visible.preview { 50 } else { 0 }),
                Constraint::Min(0),
            ])
            .areas(right);
            let application_block = Block::bordered()
                .title("Application")
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(if focused { Color::Gray } else { Color::Cyan }));
            let application_inner = application_block.inner(application);
            frame.render_widget(application_block, application);
            frame.render_widget(
                Text::from(if focused {
                    "Press Tab to focus"
                } else {
                    "Application focused\n\nPress Tab to return to the picker"
                }),
                application_inner,
            );
            let [prompt, status, list] = Layout::vertical([
                Constraint::Length(u16::from(visible.prompt)),
                Constraint::Length(u16::from(visible.status)),
                Constraint::Min(0),
            ])
            .areas(main);
            frame.render_widget(prepared.prompt(), prompt);
            frame.render_widget(prepared.match_list(), list);
            frame.render_widget(prepared.status(), status);
            let preview_block = Block::bordered()
                .title("Preview")
                .border_type(BorderType::Rounded);
            let preview_content = preview_block.inner(preview);
            if visible.preview {
                frame.render_widget(preview_block, preview);
            }
            frame.render_widget(prepared.preview(), preview_content);
            if focused && let Some(cursor) = prepared.prompt().cursor() {
                frame.set_cursor_position((prompt.x + cursor.column, prompt.y + cursor.row));
            }
        })?;
        prepared.complete();

        if event::poll(deadline.saturating_duration_since(Instant::now()))? {
            let mut batch = session.events();
            match event::read()? {
                TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Tab {
                        focused = !focused;
                    } else if key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break Exit::Quit;
                    } else if focused {
                        match keybind_single_preview::<Infallible>(key) {
                            Some(Event::Prompt(event)) => batch.prompt(event),
                            Some(Event::MatchList(event)) => batch.match_list(event),
                            Some(Event::Preview(event)) if batch.visible_components().preview => {
                                batch.preview(event);
                            }
                            Some(Event::Select) => {
                                let hovered = batch.hovered_id();
                                if hovered.is_some()
                                    || batch.queue_status().is_some_and(|queue| queue.count != 0)
                                {
                                    break Exit::Selected(hovered);
                                }
                            }
                            Some(Event::Quit | Event::UserInterrupt) => break Exit::Quit,
                            Some(Event::QuitPromptEmpty) if batch.query().is_empty() => {
                                break Exit::Quit;
                            }
                            Some(Event::Layout(LayoutEvent::TogglePreview)) => {
                                let mut visible = batch.visible_components();
                                visible.preview = !visible.preview;
                                batch.set_visible_components(visible);
                            }
                            Some(Event::Redraw) => terminal.clear()?,
                            _ => {}
                        }
                    }
                }
                TerminalEvent::Paste(text) if focused => batch.prompt(PromptEvent::Paste(text)),
                _ => {}
            }
        }
    };
    Ok(session.finish(exit).copied())
}
