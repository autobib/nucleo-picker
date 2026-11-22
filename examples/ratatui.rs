//! # Ratatui Demo
//!
//! This is an example showing how to use the session API to integrate the picker into a larger
//! application using Ratatui.
//!
//! This example assumes that you are familiar with Ratatui more generally. See the [Ratatui
//! docs](https://docs.rs/ratatui) for more detail if you are not.
//!
//! Run with
//! ```sh
//! cargo run --example ratatui --features ratatui --release
//! ```

use std::{
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
use nucleo_picker::event::{Event, LayoutEvent, keybind_single_preview};
use nucleo_picker_core::{
    event::PromptEvent,
    preview::Pane,
    render::StrRenderer,
    session::{Exit, FrameOptions, ModelOptions},
};
use nucleo_picker_interface::UncachedPreview;
use ratatui_core::{
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    terminal::Terminal,
    text::{Line, Text},
};
use ratatui_crossterm::CrosstermBackend;
use ratatui_widgets::{block::Block, borders::BorderType};

fn main() -> io::Result<()> {
    // standard IO setup and cleanup
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

    // print the selected item at the end
    if let Some(selected) = selected {
        println!("You selected '{selected}'");
    }
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<Option<&'static str>> {
    // initialize the underlying picker model
    let mut model = ModelOptions::new().list_reversed(true).model(StrRenderer);

    // initialize the preview interface
    let interface = UncachedPreview::new_single(|item: &&'static str, pane: &mut Pane| {
        pane.push_text("Selected item:\n\n");
        pane.push_text(item);
    });

    // we just manually push a few items for convenience in this example.
    // more generally, you should use the injector API to send items to the picker from a separate
    // thread
    model.push_batch(["red", "green", "blue", "yellow", "purple"]);

    // initialize a new session.
    let mut session = model.session(interface);
    let mut focused = true;
    let mut next_background = Instant::now();

    // a title and underline
    let title = Line::from("nucleo-picker Ratatui demo").bold().centered();
    let underline = Line::from("─".repeat(title.width())).dark_gray().centered();

    let exit = loop {
        // the main render loop using

        // frame timing:
        //  - 16ms frame duration for the main render
        //  - 100ms frame duration for background (low-frequency-update) events
        let now = Instant::now();
        let deadline = now + Duration::from_millis(16);
        let background = now >= next_background;
        if background {
            next_background = now + Duration::from_millis(100);
        }
        session.update_matches(Duration::from_millis(1));

        // the first step is to prepare the frame for rendering. the FrameOptions are used to pass
        // information about the background work deadline and other context for rendering the frame
        let mut prepared =
            session.prepare_frame(FrameOptions::new(deadline).background(background));

        // draw the frame itself
        terminal.draw(|frame| {
            // build-up the layout of the frame
            let [title_area, underline_area, _, content, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(frame.area());

            // render the title at the top
            frame.render_widget(&title, title_area);

            // render the title underline
            frame.render_widget(&underline, underline_area);

            // render the help text at the bottom
            frame.render_widget(
                Line::from("Tab: focus | Enter: select | Alt-P: toggle preview | Esc: quit"),
                footer,
            );

            // now, iterate over the active components and draw them. the only thing that could change is
            // visibility of the preview pane (toggled by Alt-P)
            let active = prepared.active_components();
            let [main, right] =
                Layout::horizontal([Constraint::Min(0), Constraint::Percentage(40)]).areas(content);

            // reserve space for the preview only if it is visible
            let [preview, application] = Layout::vertical([
                Constraint::Percentage(if active.preview { 50 } else { 0 }),
                Constraint::Min(0),
            ])
            .areas(right);

            // a "Application block" (just a placeholder for the example)
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

            // render the prompt, status line, and match list
            let [prompt, status, list] = Layout::vertical([
                Constraint::Length(u16::from(active.prompt)),
                Constraint::Length(u16::from(active.status)),
                Constraint::Min(0),
            ])
            .areas(main);
            frame.render_widget(prepared.prompt(), prompt);
            frame.render_widget(prepared.match_list(), list);
            frame.render_widget(prepared.status(), status);

            // render the preview pane if it is active
            let preview_block = Block::bordered()
                .title("Preview")
                .border_type(BorderType::Rounded);
            let preview_content = preview_block.inner(preview);
            if active.preview {
                frame.render_widget(preview_block, preview);
            }
            frame.render_widget(prepared.preview(), preview_content);

            // check where the focus is. if it is not on the "Application" part of the screen,
            // read the cursor position from the prompt component and use it to set the cursor
            // position
            if focused && let Some(cursor) = prepared.prompt().cursor() {
                frame.set_cursor_position((prompt.x + cursor.column, prompt.y + cursor.row));
            }
        })?;

        // report that frame rendering is done
        prepared.complete();

        // process events in the remaining frame time
        if event::poll(deadline.saturating_duration_since(Instant::now()))? {
            // for the component-specific events, we forward them to the picker, except for certain
            // events which we process here
            let mut batch = session.events();
            match event::read()? {
                TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Tab {
                        // toggle focus with TAB
                        focused = !focused;
                    } else if key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        // quit with Escape / CTRL+C
                        break Exit::Quit;
                    } else if focused {
                        // we just use the built-in keybindings for convenience mapping of
                        // events
                        match keybind_single_preview::<!>(key) {
                            // forward the prompt, match-list, and preview events to the picker
                            Some(Event::Prompt(event)) => batch.prompt(event),
                            Some(Event::MatchList(event)) => batch.match_list(event),
                            Some(Event::Preview(event)) if batch.active_components().preview => {
                                // we skip preview events if the preview pane is hidden
                                batch.preview(event);
                            }
                            Some(Event::Select) => {
                                // check if the there is either a hovered item (that we can select)
                                // or if there any queued items to select. if so, exit with the
                                // selection
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
                                // the default keybindings convert Alt+P to the TogglePreview event
                                let mut active = batch.active_components();
                                active.preview.toggle();
                                batch.set_active_components(active);
                            }
                            // the built-in keybindings report a redraw whenever the terminal size
                            // hanges, which means we have to do a full redraw
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
