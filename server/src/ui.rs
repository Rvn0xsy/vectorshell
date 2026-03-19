use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct UiState {
    pub waiting_for_input: bool,
    pub prompt: String,
}

pub fn prompt_line() -> String {
    format!("{}  You> ", now_time())
}

pub fn set_waiting(ui: &Arc<Mutex<UiState>>, waiting: bool) {
    if let Ok(mut guard) = ui.lock() {
        guard.waiting_for_input = waiting;
    }
}

pub fn set_prompt(ui: &Arc<Mutex<UiState>>, prompt: String) {
    if let Ok(mut guard) = ui.lock() {
        guard.prompt = prompt;
    }
}

pub fn ui_print(ui: &Arc<Mutex<UiState>>, role: &str, message: &str) {
    let (waiting, prompt) = if let Ok(guard) = ui.lock() {
        (guard.waiting_for_input, guard.prompt.clone())
    } else {
        (false, String::new())
    };

    if waiting {
        println!();
    }
    println!("{}  {:<6} {}", now_time(), role, message);
    if waiting {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
    }
}

fn now_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
