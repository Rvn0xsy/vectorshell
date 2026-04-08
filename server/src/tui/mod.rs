pub mod app;
pub mod input;
pub mod render;
pub mod theme;

use crate::agent::Agent;
use crate::client_manager::ClientManager;
use crate::db::Db;
use crate::ui::UiState;
use app::{PanelFocus, TuiApp};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub enum TuiEvent {
    Output { role: String, message: String },
    /// Agent response with generation counter to discard stale responses after session switch
    AgentResponse { message: String, generation: u64 },
    SessionsChanged,
    Notify(String),
    Quit,
}

pub type TuiEventSender = mpsc::UnboundedSender<TuiEvent>;

pub async fn run_tui(
    manager: Arc<Mutex<ClientManager>>,
    db: Arc<Mutex<Db>>,
    agent: Agent,
    ui_state: Arc<Mutex<UiState>>,
) {
    let (tui_tx, tui_rx) = mpsc::unbounded_channel();

    // Store sender in UiState so ui_print can use it
    if let Ok(mut guard) = ui_state.lock() {
        guard.tui_tx = Some(tui_tx.clone());
    }

    let mut app = TuiApp::new(manager, db, agent, ui_state, tui_tx, tui_rx);
    app.refresh_sessions();
    app.push_output("Info", "VectorShell TUI started. Type /help for commands.");

    if let Err(e) = run_tui_loop(&mut app).await {
        // Restore terminal before printing error
        let _ = restore_terminal();
        eprintln!("TUI error: {e}");
    }
}

async fn run_tui_loop(app: &mut TuiApp) -> io::Result<()> {
    setup_terminal()?;

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut last_session_refresh = Instant::now();

    loop {
        // 1. Render
        terminal.draw(|f| render::render(f, app))?;

        // 2. Poll crossterm events (50ms timeout)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(app, key);
            }
        }

        // 3. Drain TUI events from channel
        while let Ok(evt) = app.tui_rx.try_recv() {
            app.handle_tui_event(evt);
        }

        // 4. Periodic session refresh (every 2 seconds)
        if last_session_refresh.elapsed() >= Duration::from_secs(2) {
            app.refresh_sessions();
            last_session_refresh = Instant::now();
        }

        // 5. Check quit
        if app.should_quit {
            break;
        }
    }

    restore_terminal()?;
    Ok(())
}

fn handle_key_event(app: &mut TuiApp, key: KeyEvent) {
    // Only handle key press events, ignore release/repeat
    if key.kind != KeyEventKind::Press {
        return;
    }

    // Global keys
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    if key.code == KeyCode::Tab {
        if app.focus == PanelFocus::Input {
            if app.popup_visible() {
                // Accept popup selection
                app.popup_accept();
            } else if app.input.buffer().starts_with('/') {
                // Tab completion for /use <install_id> etc.
                app.handle_tab_completion();
            } else {
                app.cycle_focus();
            }
        } else {
            app.cycle_focus();
        }
        return;
    }

    if key.code == KeyCode::Esc {
        app.focus = PanelFocus::Input;
        return;
    }

    match app.focus {
        PanelFocus::Input => handle_input_key(app, key),
        PanelFocus::Sessions => handle_sessions_key(app, key),
        PanelFocus::Output => handle_output_key(app, key),
    }
}

fn handle_input_key(app: &mut TuiApp, key: KeyEvent) {
    let popup_visible = app.popup_visible();

    match key.code {
        KeyCode::Enter => {
            if popup_visible {
                // Accept popup selection and stay in input
                app.popup_accept();
            } else {
                let input = app.input.submit();
                let trimmed = input.trim().to_string();
                if !trimmed.is_empty() {
                    app.handle_command(&trimmed);
                }
            }
        }
        KeyCode::Char(c) => {
            app.input.insert_char(c);
            app.reset_popup();
        }
        KeyCode::Backspace => {
            app.input.backspace();
            app.reset_popup();
        }
        KeyCode::Delete => {
            app.input.delete();
            app.reset_popup();
        }
        KeyCode::Left => {
            app.input.move_left();
        }
        KeyCode::Right => {
            app.input.move_right();
        }
        KeyCode::Home => {
            app.input.home();
        }
        KeyCode::End => {
            app.input.end();
        }
        KeyCode::Up => {
            if popup_visible {
                app.popup_up();
            } else {
                app.input.history_up();
            }
        }
        KeyCode::Down => {
            if popup_visible {
                app.popup_down();
            } else {
                app.input.history_down();
            }
        }
        _ => {}
    }
}

fn handle_sessions_key(app: &mut TuiApp, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.sessions_selected > 0 {
                app.sessions_selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.sessions_selected + 1 < app.sessions_cache.len() {
                app.sessions_selected += 1;
            }
        }
        KeyCode::Enter => {
            app.select_session_by_index();
            // After selecting, switch focus to Input for immediate interaction
            app.focus = PanelFocus::Input;
        }
        // Number keys 1-9 for quick selection
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            if idx < app.sessions_cache.len() {
                app.sessions_selected = idx;
                app.select_session_by_index();
                app.focus = PanelFocus::Input;
            }
        }
        _ => {}
    }
}

fn handle_output_key(app: &mut TuiApp, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            app.output_scroll = app.output_scroll.saturating_add(1);
        }
        KeyCode::Down => {
            app.output_scroll = app.output_scroll.saturating_sub(1);
        }
        KeyCode::PageUp => {
            app.output_scroll = app.output_scroll.saturating_add(20);
        }
        KeyCode::PageDown => {
            app.output_scroll = app.output_scroll.saturating_sub(20);
        }
        KeyCode::Home | KeyCode::Char('g') => {
            // Scroll to top — use large value, render will clamp to actual max
            app.output_scroll = usize::MAX / 2;
        }
        KeyCode::End | KeyCode::Char('G') => {
            // Scroll to bottom
            app.output_scroll = 0;
        }
        _ => {}
    }
    // Note: precise clamping happens in render_output_panel based on visual rows
}

fn setup_terminal() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

fn restore_terminal() -> io::Result<()> {
    terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
