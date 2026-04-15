use crate::tui::{TuiEvent, TuiEventSender};
use std::sync::{Arc, Mutex};

pub struct UiState {
    pub tui_tx: Option<TuiEventSender>,
}

impl Default for UiState {
    fn default() -> Self {
        Self { tui_tx: None }
    }
}

impl std::fmt::Debug for UiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("tui_tx", &self.tui_tx.is_some())
            .finish()
    }
}

pub fn ui_print(ui: &Arc<Mutex<UiState>>, role: &str, message: &str) {
    if let Ok(guard) = ui.lock() {
        if let Some(tx) = &guard.tui_tx {
            let _ = tx.send(TuiEvent::Output {
                role: role.to_string(),
                message: message.to_string(),
            });
        }
    }
}
