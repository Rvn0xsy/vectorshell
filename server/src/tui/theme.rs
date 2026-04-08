use ratatui::style::{Color, Modifier, Style};

pub fn role_style(role: &str) -> Style {
    match role {
        "Info" => Style::default().fg(Color::Cyan),
        "Exec" => Style::default().fg(Color::Yellow),
        "Result" => Style::default().fg(Color::Green),
        "Agent" => Style::default().fg(Color::Magenta),
        "Tool" => Style::default().fg(Color::Blue),
        "Error" => Style::default().fg(Color::Red),
        _ => Style::default(),
    }
}

pub fn timestamp_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn status_bar_style() -> Style {
    Style::default().fg(Color::White).bg(Color::DarkGray)
}

pub fn focused_border_style() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn unfocused_border_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn selected_session_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

pub fn online_marker_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn input_prompt_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}
