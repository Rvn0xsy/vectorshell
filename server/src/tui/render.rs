use crate::tui::app::{CommandHint, OutputLine, PanelFocus, TuiApp};
use crate::tui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub fn render(frame: &mut Frame, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(5),   // middle
            Constraint::Length(1), // input bar
        ])
        .split(frame.area());

    render_status_bar(frame, app, chunks[0]);
    render_middle(frame, app, chunks[1]);
    render_input_bar(frame, app, chunks[2]);

    // Render command popup overlay (on top of everything)
    let hints = app.get_popup_hints();
    if app.focus == PanelFocus::Input && !hints.is_empty() {
        render_command_popup(frame, app, &hints, chunks[2]);
    }
}

fn render_status_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let selected = app
        .selected_install_id()
        .unwrap_or_else(|| "none".to_string());

    let mode = if app.agent_mode { "Agent" } else { "Manual" };

    let focus_name = match app.focus {
        PanelFocus::Input => "Input",
        PanelFocus::Sessions => "Sessions",
        PanelFocus::Output => "Output",
    };

    let spans = vec![
        Span::styled(" VectorShell ", theme::status_bar_style().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" | Sessions: {} ", app.session_count()),
            theme::status_bar_style(),
        ),
        Span::styled(
            format!(" | Target: {} ", truncate(&selected, 20)),
            theme::status_bar_style(),
        ),
        Span::styled(format!(" | {} ", mode), theme::status_bar_style()),
        Span::styled(
            format!(" | Focus: {} ", focus_name),
            theme::status_bar_style().add_modifier(Modifier::BOLD),
        ),
    ];

    let status = Paragraph::new(Line::from(spans)).style(theme::status_bar_style());
    frame.render_widget(status, area);
}

fn render_middle(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_sessions_panel(frame, app, chunks[0]);
    render_output_panel(frame, app, chunks[1]);
}

fn render_sessions_panel(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let border_style = if app.focus == PanelFocus::Sessions {
        theme::focused_border_style()
    } else {
        theme::unfocused_border_style()
    };

    let title = " Sessions ";

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = app
        .sessions_cache
        .iter()
        .enumerate()
        .map(|(i, client)| {
            let is_active = app
                .selected_client
                .as_ref()
                .map_or(false, |s| *s == client.session_id);

            let marker = if is_active { ">" } else { " " };
            let num = if i < 9 {
                format!("{}", i + 1)
            } else {
                " ".to_string()
            };

            let style = if i == app.sessions_selected && app.focus == PanelFocus::Sessions {
                theme::selected_session_style()
            } else if is_active {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            // Show more useful info: number, marker, hostname, user@os, short install_id
            let install_short: String = client.install_id.chars().take(8).collect();
            let text = format!(
                "{}{} {} {}@{} [{}]",
                num, marker, client.hostname, client.username, client.os, install_short
            );
            ListItem::new(text).style(style)
        })
        .collect();

    // Use ListState with highlight so ratatui auto-scrolls to the selected item
    let highlight_style = if app.focus == PanelFocus::Sessions {
        theme::selected_session_style()
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style);

    let mut list_state = ListState::default();
    if !app.sessions_cache.is_empty() {
        list_state.select(Some(app.sessions_selected));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_output_panel(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let border_style = if app.focus == PanelFocus::Output {
        theme::focused_border_style()
    } else {
        theme::unfocused_border_style()
    };

    let total_lines = app.output_lines.len();

    // Show scroll position in title when scrolled up
    let title = if app.output_scroll > 0 {
        format!(" Output [{} lines, +{}] ", total_lines, app.output_scroll)
    } else {
        format!(" Output [{} lines] ", total_lines)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner_area = block.inner(area);
    let visible_height = inner_area.height as usize;
    let inner_width = inner_area.width.max(1) as usize;

    // Format all lines (no truncation — Wrap handles long lines)
    let lines: Vec<Line> = app
        .output_lines
        .iter()
        .map(|line| format_output_line(line))
        .collect();

    // Calculate total visual rows accounting for line wrapping
    let total_visual_rows: usize = lines
        .iter()
        .map(|line| {
            let w = line.width();
            if w == 0 {
                1
            } else {
                (w + inner_width - 1) / inner_width
            }
        })
        .sum();

    // Scroll calculation in visual rows
    let max_scroll = total_visual_rows.saturating_sub(visible_height);
    let effective_scroll = app.output_scroll.min(max_scroll);
    // scroll offset from top (visual rows)
    let scroll_from_top = max_scroll.saturating_sub(effective_scroll);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_from_top as u16, 0));
    frame.render_widget(paragraph, area);

    // Scrollbar
    if total_visual_rows > visible_height {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll).position(scroll_from_top);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn format_output_line(line: &OutputLine) -> Line<'static> {
    let timestamp = format!("{} ", line.timestamp);
    let role = format!("{:<6} ", line.role);

    let timestamp_span = Span::styled(timestamp, theme::timestamp_style());
    let role_span = Span::styled(role, theme::role_style(&line.role));
    let message_span = Span::raw(line.message.clone());

    Line::from(vec![timestamp_span, role_span, message_span])
}

fn render_input_bar(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let prompt = if app.agent_mode {
        "Agent> "
    } else {
        "You> "
    };

    let prompt_style = if app.focus == PanelFocus::Input {
        theme::input_prompt_style()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut spans = vec![
        Span::styled(prompt, prompt_style),
        Span::raw(app.input.buffer().to_string()),
    ];

    // Show completion hint
    if let Some(ref hint) = app.input.completion_hint {
        spans.push(Span::styled(
            format!(" {}", hint),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let input_line = Paragraph::new(Line::from(spans));
    frame.render_widget(input_line, area);

    // Set cursor position only when input is focused
    if app.focus == PanelFocus::Input {
        let cursor_x = area.x + prompt.len() as u16 + app.input.cursor_display_col() as u16;
        let cursor_y = area.y;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_command_popup(
    frame: &mut Frame,
    app: &TuiApp,
    hints: &[&'static CommandHint],
    input_area: Rect,
) {
    let item_count = hints.len();
    // Popup height: items + 2 for border
    let popup_height = (item_count as u16 + 2).min(16);
    // Popup width: fit the longest item, minimum 40
    let max_item_width = hints
        .iter()
        .map(|h| h.command.len() + h.description.len() + 4) // "  command  description"
        .max()
        .unwrap_or(30) as u16;
    let popup_width = (max_item_width + 4).min(input_area.width).max(40);

    // Position: above the input bar, left-aligned with prompt
    let prompt_len = if app.agent_mode { 7u16 } else { 5u16 }; // "Agent> " or "You> "
    let popup_x = input_area.x + prompt_len;
    let popup_y = input_area.y.saturating_sub(popup_height);

    // Clamp to screen bounds
    let popup_x = popup_x.min(frame.area().width.saturating_sub(popup_width));

    let area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let items: Vec<ListItem> = hints
        .iter()
        .enumerate()
        .map(|(i, hint)| {
            let style = if i == app.popup_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::Black)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {:<12}", hint.command),
                    style,
                ),
                Span::styled(
                    format!(" {}", hint.description),
                    if i == app.popup_selected {
                        style
                    } else {
                        Style::default().fg(Color::DarkGray).bg(Color::Black)
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn truncate(s: &str, max_len: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w > max_len {
        let mut cur = 0;
        let truncated: String = s.chars().take_while(|c| {
            cur += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
            cur <= max_len
        }).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}
