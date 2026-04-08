use crate::agent::Agent;
use crate::client_manager::ClientManager;
use crate::db::Db;
use crate::tui::input::InputState;
use crate::tui::TuiEvent;
use crate::ui::UiState;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::Local;
use tokio::sync::mpsc;

const MAX_OUTPUT_LINES: usize = 10_000;

#[derive(Debug, Clone)]
pub struct OutputLine {
    pub timestamp: String,
    pub role: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    Input,
    Sessions,
    Output,
}

#[derive(Debug, Default)]
pub struct AgentContext {
    history: Vec<String>,
}

impl AgentContext {
    pub fn push(&mut self, entry: String) {
        self.history.push(entry);
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }

    pub fn as_prompt(&self) -> String {
        if self.history.is_empty() {
            String::new()
        } else {
            format!("Context:\n{}\n", self.history.join("\n"))
        }
    }
}

pub struct TuiApp {
    pub manager: Arc<Mutex<ClientManager>>,
    pub db: Arc<Mutex<Db>>,
    pub agent: Agent,
    pub ui_state: Arc<Mutex<UiState>>,

    pub output_lines: VecDeque<OutputLine>,
    pub output_scroll: usize,

    pub sessions_cache: Vec<crate::client_manager::ClientMetadata>,
    pub sessions_scroll: usize,
    pub sessions_selected: usize,

    pub selected_client: Option<String>,
    pub agent_mode: bool,
    pub context: AgentContext,

    pub input: InputState,
    pub focus: PanelFocus,

    pub tui_tx: mpsc::UnboundedSender<TuiEvent>,
    pub tui_rx: mpsc::UnboundedReceiver<TuiEvent>,

    pub should_quit: bool,

    // Command hint popup state
    pub popup_selected: usize,

    // Generation counter: incremented on session switch to discard stale agent responses
    pub agent_generation: u64,
}

/// A command hint entry for the popup.
#[derive(Debug, Clone)]
pub struct CommandHint {
    pub command: &'static str,
    pub description: &'static str,
}

pub const COMMAND_HINTS: &[CommandHint] = &[
    CommandHint { command: "/sessions",  description: "List connected clients" },
    CommandHint { command: "/use",       description: "Select a client by install_id" },
    CommandHint { command: "/info",      description: "Show selected session details" },
    CommandHint { command: "/exec",      description: "Execute command on client" },
    CommandHint { command: "/tool",      description: "Dispatch generic tool call" },
    CommandHint { command: "/read",      description: "Read file from client" },
    CommandHint { command: "/write",     description: "Write file to client" },
    CommandHint { command: "/upload",    description: "Upload file to client" },
    CommandHint { command: "/download",  description: "Download file from client" },
    CommandHint { command: "/agent",     description: "Ask AI (text only)" },
    CommandHint { command: "/clean",     description: "Clear install history + context" },
    CommandHint { command: "/clear",     description: "Clear agent context" },
    CommandHint { command: "/back",      description: "Exit agent mode" },
    CommandHint { command: "/help",      description: "Show help" },
];

impl TuiApp {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        db: Arc<Mutex<Db>>,
        agent: Agent,
        ui_state: Arc<Mutex<UiState>>,
        tui_tx: mpsc::UnboundedSender<TuiEvent>,
        tui_rx: mpsc::UnboundedReceiver<TuiEvent>,
    ) -> Self {
        Self {
            manager,
            db,
            agent,
            ui_state,
            output_lines: VecDeque::new(),
            output_scroll: 0,
            sessions_cache: Vec::new(),
            sessions_scroll: 0,
            sessions_selected: 0,
            selected_client: None,
            agent_mode: false,
            context: AgentContext::default(),
            input: InputState::new(),
            focus: PanelFocus::Input,
            tui_tx,
            tui_rx,
            should_quit: false,
            popup_selected: 0,
            agent_generation: 0,
        }
    }

    pub fn push_output(&mut self, role: &str, message: &str) {
        let timestamp = now_time();
        // Handle multi-line messages
        for line in message.lines() {
            self.output_lines.push_back(OutputLine {
                timestamp: timestamp.clone(),
                role: role.to_string(),
                message: line.to_string(),
            });
        }
        // If message has no lines (empty), still push one entry
        if message.is_empty() {
            self.output_lines.push_back(OutputLine {
                timestamp,
                role: role.to_string(),
                message: String::new(),
            });
        }
        // Evict old lines, adjust scroll to compensate
        while self.output_lines.len() > MAX_OUTPUT_LINES {
            self.output_lines.pop_front();
            // If user was scrolled up, keep their position stable
            self.output_scroll = self.output_scroll.saturating_sub(1);
        }
        // Auto-scroll to bottom if near bottom
        if self.output_scroll <= 2 {
            self.output_scroll = 0;
        }
    }

    pub fn refresh_sessions(&mut self) {
        if let Ok(mgr) = self.manager.lock() {
            self.sessions_cache = mgr.list_clients();
        }
        // Clamp selected index to valid range
        if !self.sessions_cache.is_empty() {
            if self.sessions_selected >= self.sessions_cache.len() {
                self.sessions_selected = self.sessions_cache.len() - 1;
            }
        } else {
            self.sessions_selected = 0;
        }
    }

    pub fn handle_tui_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Output { role, message } => {
                self.push_output(&role, &message);
            }
            TuiEvent::AgentResponse { message, generation } => {
                self.push_output("Agent", &message);
                // Only add to context if still on the same session (generation matches)
                if generation == self.agent_generation {
                    self.context.push(format!("Assistant: {}", message));
                }
            }
            TuiEvent::SessionsChanged => {
                self.refresh_sessions();
            }
            TuiEvent::Notify(msg) => {
                self.push_output("Info", &msg);
            }
            TuiEvent::Quit => {
                self.should_quit = true;
            }
        }
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            PanelFocus::Input => PanelFocus::Sessions,
            PanelFocus::Sessions => PanelFocus::Output,
            PanelFocus::Output => PanelFocus::Input,
        };
    }

    pub fn session_count(&self) -> usize {
        self.sessions_cache.len()
    }

    pub fn selected_install_id(&self) -> Option<String> {
        let session_id = self.selected_client.as_ref()?;
        if let Ok(mgr) = self.manager.lock() {
            mgr.get_client_metadata(session_id).map(|m| m.install_id)
        } else {
            None
        }
    }

    pub fn handle_command(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }

        if trimmed == "/help" {
            self.print_help();
            return;
        }

        if trimmed == "/sessions" {
            self.refresh_sessions();
            if self.sessions_cache.is_empty() {
                self.push_output("Info", "No clients connected");
            } else {
                self.push_output("Info", &format!("{} client(s) connected:", self.sessions_cache.len()));
                for (i, client) in self.sessions_cache.clone().iter().enumerate() {
                    self.push_output("Info", &format!(
                        "  [{}] {} | {}@{} | {} | {}",
                        i + 1, client.hostname, client.username, client.os,
                        client.install_id, client.ip
                    ));
                }
            }
            return;
        }

        if trimmed == "/info" {
            if let Some(client_id) = self.selected_client.clone() {
                let info = self.manager.lock().ok().and_then(|mgr| {
                    mgr.get_client_metadata(&client_id).map(|meta| {
                        vec![
                            format!("  Hostname:    {}", meta.hostname),
                            format!("  User:        {}", meta.username),
                            format!("  OS/Arch:     {}/{}", meta.os, meta.arch),
                            format!("  IP:          {}", meta.ip),
                            format!("  PID:         {}", meta.pid),
                            format!("  Install ID:  {}", meta.install_id),
                            format!("  Session ID:  {}", meta.session_id),
                            format!("  Build UUID:  {}", meta.build_uuid),
                            format!("  Heartbeat:   {}", meta.last_heartbeat),
                        ]
                    })
                });
                match info {
                    Some(lines) => {
                        self.push_output("Info", "Client details:");
                        for line in lines {
                            self.push_output("Info", &line);
                        }
                    }
                    None => self.push_output("Error", "Selected client not found"),
                }
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if trimmed == "/clean" {
            self.context.clear();
            if let Some(client_id) = self.selected_client.clone() {
                let install_id = self
                    .manager
                    .lock()
                    .ok()
                    .and_then(|mgr| mgr.get_client_metadata(&client_id))
                    .map(|m| m.install_id)
                    .unwrap_or_default();
                if !install_id.is_empty() {
                    if let Ok(db) = self.db.lock() {
                        let _ = db.clear_install_history(&install_id);
                    }
                }
            }
            self.push_output("Info", "History cleared");
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/use ") {
            let wanted = rest.trim();
            let mapped = self
                .manager
                .lock()
                .ok()
                .and_then(|mgr| mgr.get_by_install_id(wanted))
                .map(|m| m.session_id);
            if let Some(session_id) = mapped {
                self.selected_client = Some(session_id);
                self.agent_mode = true;
                self.context.clear();
                self.agent_generation += 1;
                self.push_output("Info", &format!("Selected client: {}", wanted));
                // Update sessions_selected index
                if let Some(idx) = self
                    .sessions_cache
                    .iter()
                    .position(|s| s.install_id == wanted)
                {
                    self.sessions_selected = idx;
                }
            } else {
                self.push_output("Error", "Client not found, use /sessions to list");
            }
            return;
        }

        if trimmed == "/back" {
            self.selected_client = None;
            self.agent_mode = false;
            self.context.clear();
            self.agent_generation += 1;
            self.push_output("Info", "Returned to top level");
            return;
        }

        if trimmed == "/clear" {
            self.context.clear();
            self.agent_generation += 1;
            self.push_output("Info", "Context cleared");
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/exec ") {
            if let Some(client_id) = self.selected_client.clone() {
                let cmd = rest.trim();
                self.push_output("Exec", cmd);
                let err_msg = self
                    .manager
                    .lock()
                    .map(|mgr| mgr.send_exec(&client_id, cmd))
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|r| r.map_err(|e| e))
                    .err();
                if let Some(error) = err_msg {
                    self.push_output("Error", &format!("failed to send exec: {error}"));
                }
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/tool ") {
            if let Some(client_id) = self.selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let tool_name = parts.next().unwrap_or("").trim();
                if tool_name.is_empty() {
                    self.push_output("Error", "Usage: /tool <tool_name> <json_args>");
                    return;
                }
                let args_raw = parts.next().unwrap_or("{}").trim();
                let args = match serde_json::from_str::<serde_json::Value>(args_raw) {
                    Ok(value) => value,
                    Err(error) => {
                        self.push_output("Error", &format!("invalid JSON args: {error}"));
                        return;
                    }
                };
                self.context
                    .push(format!("ManualTool: {} {}", tool_name, args));
                self.append_manual_tool_history(&client_id, tool_name, &args);
                self.spawn_tool_dispatch(client_id, tool_name.to_string(), args);
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(path) = trimmed.strip_prefix("/read ") {
            if let Some(client_id) = self.selected_client.clone() {
                let path = path.trim();
                if path.is_empty() {
                    self.push_output("Error", "Usage: /read <path>");
                    return;
                }
                let args = serde_json::json!({ "path": path });
                self.context.push(format!("ManualTool: read_file {}", args));
                self.append_manual_tool_history(&client_id, "read_file", &args);
                self.spawn_tool_dispatch(client_id, "read_file".to_string(), args);
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/write ") {
            if let Some(client_id) = self.selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let path = parts.next().unwrap_or("").trim();
                let content = parts.next().unwrap_or("");
                if path.is_empty() {
                    self.push_output("Error", "Usage: /write <path> <content>");
                    return;
                }
                let args = serde_json::json!({ "path": path, "content": content });
                self.context
                    .push(format!("ManualTool: write_file {}", args));
                self.append_manual_tool_history(&client_id, "write_file", &args);
                self.spawn_tool_dispatch(client_id, "write_file".to_string(), args);
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/upload ") {
            if let Some(client_id) = self.selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let src = parts.next().unwrap_or("").trim().to_string();
                let dst = parts.next().unwrap_or("").trim().to_string();
                if src.is_empty() || dst.is_empty() {
                    self.push_output("Error", "Usage: /upload <local_path> <remote_path>");
                    return;
                }
                self.push_output("Upload", &format!("{} -> {}", src, dst));
                let manager = Arc::clone(&self.manager);
                let tx = self.tui_tx.clone();
                tokio::spawn(async move {
                    const CHUNK_SIZE: usize = 256 * 1024;
                    let bytes = match tokio::fs::read(&src).await {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Error".into(),
                                message: format!("read local file failed: {e}"),
                            });
                            return;
                        }
                    };
                    let total_size = bytes.len();
                    let chunks: Vec<String> = bytes
                        .chunks(CHUNK_SIZE)
                        .map(|chunk| BASE64.encode(chunk))
                        .collect();
                    let total_chunks = chunks.len();
                    for (idx, chunk) in chunks.into_iter().enumerate() {
                        let payload = serde_json::json!({
                            "path": dst,
                            "content_base64": chunk,
                            "append": idx > 0,
                        });
                        let receiver = {
                            let mut mgr = match manager.lock() {
                                Ok(m) => m,
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Output {
                                        role: "Error".into(),
                                        message: format!("manager lock failed: {e}"),
                                    });
                                    return;
                                }
                            };
                            match mgr.dispatch_tool_call(&client_id, "upload_file", payload, Some(60_000)) {
                                Ok((_, rx)) => rx,
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Output {
                                        role: "Error".into(),
                                        message: format!("upload dispatch failed: {e}"),
                                    });
                                    return;
                                }
                            }
                        };
                        match tokio::time::timeout(std::time::Duration::from_secs(65), receiver).await {
                            Ok(Ok(result)) if result.ok => {
                                if total_chunks > 1 {
                                    let _ = tx.send(TuiEvent::Output {
                                        role: "Upload".into(),
                                        message: format!("chunk {}/{} ok", idx + 1, total_chunks),
                                    });
                                }
                            }
                            Ok(Ok(result)) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: format!("upload chunk {} failed: {}", idx + 1, result.error),
                                });
                                return;
                            }
                            Ok(Err(_)) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: "upload: result channel closed".into(),
                                });
                                return;
                            }
                            Err(_) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: "upload: timed out".into(),
                                });
                                return;
                            }
                        }
                    }
                    let _ = tx.send(TuiEvent::Output {
                        role: "Upload".into(),
                        message: format!("ok, {} bytes in {} chunk(s)", total_size, total_chunks),
                    });
                });
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/download ") {
            if let Some(client_id) = self.selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let src = parts.next().unwrap_or("").trim().to_string();
                let dst = parts.next().unwrap_or("").trim().to_string();
                if src.is_empty() || dst.is_empty() {
                    self.push_output("Error", "Usage: /download <remote_path> <local_path>");
                    return;
                }
                self.push_output("Download", &format!("{} -> {}", src, dst));
                let manager = Arc::clone(&self.manager);
                let tx = self.tui_tx.clone();
                tokio::spawn(async move {
                    const CHUNK_SIZE: usize = 256 * 1024;
                    let mut offset: usize = 0;
                    let mut bytes = Vec::new();
                    loop {
                        let payload = serde_json::json!({
                            "path": src,
                            "offset": offset,
                            "limit": CHUNK_SIZE,
                        });
                        let receiver = {
                            let mut mgr = match manager.lock() {
                                Ok(m) => m,
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Output {
                                        role: "Error".into(),
                                        message: format!("manager lock failed: {e}"),
                                    });
                                    return;
                                }
                            };
                            match mgr.dispatch_tool_call(&client_id, "download_file_chunk", payload, Some(60_000)) {
                                Ok((_, rx)) => rx,
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Output {
                                        role: "Error".into(),
                                        message: format!("download dispatch failed: {e}"),
                                    });
                                    return;
                                }
                            }
                        };
                        let result = match tokio::time::timeout(std::time::Duration::from_secs(65), receiver).await {
                            Ok(Ok(r)) => r,
                            Ok(Err(_)) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: "download: result channel closed".into(),
                                });
                                return;
                            }
                            Err(_) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: "download: timed out".into(),
                                });
                                return;
                            }
                        };
                        if !result.ok {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Error".into(),
                                message: format!("download failed: {}", result.error),
                            });
                            return;
                        }
                        let data = &result.data;
                        let inner = data.get("result").unwrap_or(data);
                        let content_b64 = inner.get("content_base64").and_then(|v| v.as_str()).unwrap_or("");
                        let chunk = match BASE64.decode(content_b64.as_bytes()) {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = tx.send(TuiEvent::Output {
                                    role: "Error".into(),
                                    message: format!("base64 decode failed: {e}"),
                                });
                                return;
                            }
                        };
                        let eof = inner.get("eof").and_then(|v| v.as_bool()).unwrap_or(true);
                        let read_len = inner.get("bytes_read").and_then(|v| v.as_u64()).unwrap_or(chunk.len() as u64) as usize;
                        if read_len == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk[..read_len.min(chunk.len())]);
                        offset += read_len;
                        if eof {
                            break;
                        }
                    }
                    // Write to local file
                    if let Some(parent) = std::path::Path::new(&dst).parent().filter(|p| !p.as_os_str().is_empty()) {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Error".into(),
                                message: format!("create directory failed: {e}"),
                            });
                            return;
                        }
                    }
                    match tokio::fs::write(&dst, &bytes).await {
                        Ok(_) => {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Download".into(),
                                message: format!("ok, {} bytes saved to {}", bytes.len(), dst),
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Error".into(),
                                message: format!("write local file failed: {e}"),
                            });
                        }
                    }
                });
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("/agent ") {
            let prompt = self.build_agent_prompt(rest.trim());
            let agent = self.agent.clone();
            let tx = self.tui_tx.clone();
            let db = Arc::clone(&self.db);
            let manager = Arc::clone(&self.manager);
            let client_id = self.selected_client.clone();
            let user_input = rest.trim().to_string();
            let generation = self.agent_generation;
            tokio::spawn(async move {
                match agent.respond_text(&prompt).await {
                    Ok(answer) => {
                        let _ = tx.send(TuiEvent::AgentResponse {
                            message: answer.clone(),
                            generation,
                        });
                        save_chat(&db, &manager, &client_id, &user_input, &answer);
                    }
                    Err(error) => {
                        let _ = tx.send(TuiEvent::Output {
                            role: "Error".into(),
                            message: format!("agent error: {error}"),
                        });
                    }
                }
            });
            self.context
                .push(format!("User: {}", rest.trim()));
            return;
        }

        // Agent mode: free text
        if self.agent_mode {
            if let Some(client_id) = self.selected_client.clone() {
                let prompt = self.build_agent_prompt(trimmed);
                let agent = self.agent.clone();
                let manager = Arc::clone(&self.manager);
                let ui_state = Arc::clone(&self.ui_state);
                let tx = self.tui_tx.clone();
                let db = Arc::clone(&self.db);
                let client_id_for_chat = client_id.clone();
                let user_input = trimmed.to_string();
                let generation = self.agent_generation;
                tokio::spawn(async move {
                    match agent
                        .respond_with_tools(&prompt, Arc::clone(&manager), &client_id, ui_state, None)
                        .await
                    {
                        Ok(answer) => {
                            let _ = tx.send(TuiEvent::AgentResponse {
                                message: answer.clone(),
                                generation,
                            });
                            save_chat(&db, &manager, &Some(client_id_for_chat), &user_input, &answer);
                        }
                        Err(error) => {
                            let _ = tx.send(TuiEvent::Output {
                                role: "Error".into(),
                                message: format!("agent error: {error}"),
                            });
                        }
                    }
                });
                self.context.push(format!("User: {}", trimmed));
            } else {
                self.push_output("Error", "No client selected, use /use <install_id>");
            }
            return;
        }

        self.push_output("Info", "Unknown command, use /help");
    }

    fn print_help(&mut self) {
        self.push_output("Info", "Commands:");
        self.push_output("Info", "  /sessions             List connected clients");
        self.push_output("Info", "  /use <install_id>     Select client (enters agent mode)");
        self.push_output("Info", "  /info                 Show selected client details");
        self.push_output("Info", "  /exec <command>       Execute command on client");
        self.push_output("Info", "  /tool <name> <json>   Dispatch tool call");
        self.push_output("Info", "  /read <path>          Read file from client");
        self.push_output("Info", "  /write <path> <text>  Write file to client");
        self.push_output("Info", "  /upload <src> <dst>   Upload file to client");
        self.push_output("Info", "  /download <src> <dst> Download file from client");
        self.push_output("Info", "  /agent <prompt>       Ask AI (text only)");
        self.push_output("Info", "  /clean                Clear install history");
        self.push_output("Info", "  /clear                Clear agent context");
        self.push_output("Info", "  /back                 Exit agent mode");
        self.push_output("Info", "  /help                 Show this help");
        self.push_output("Info", "");
        self.push_output("Info", "Keys:");
        self.push_output("Info", "  Tab       Autocomplete (/) or cycle panels");
        self.push_output("Info", "  Esc       Focus input");
        self.push_output("Info", "  Ctrl+C    Quit");
        self.push_output("Info", "  Up/Down   Scroll output or navigate sessions");
        self.push_output("Info", "  PgUp/PgDn Page scroll in output");
        self.push_output("Info", "  g/G       Jump to top/bottom of output");
    }

    fn spawn_tool_dispatch(
        &self,
        client_id: String,
        tool_name: String,
        args: serde_json::Value,
    ) {
        let manager = Arc::clone(&self.manager);
        let tx = self.tui_tx.clone();
        tokio::spawn(async move {
            let receiver = {
                let mut mgr = match manager.lock() {
                    Ok(mgr) => mgr,
                    Err(error) => {
                        let _ = tx.send(TuiEvent::Output {
                            role: "Error".into(),
                            message: format!("manager lock failed: {error}"),
                        });
                        return;
                    }
                };
                match mgr.dispatch_tool_call(&client_id, &tool_name, args, Some(60_000)) {
                    Ok((_, rx)) => rx,
                    Err(error) => {
                        let _ = tx.send(TuiEvent::Output {
                            role: "Error".into(),
                            message: format!("dispatch failed: {error}"),
                        });
                        return;
                    }
                }
            };
            match tokio::time::timeout(std::time::Duration::from_secs(65), receiver).await {
                Ok(Ok(result)) => {
                    if result.ok {
                        let rendered = serde_json::to_string_pretty(&result.data)
                            .unwrap_or_else(|_| result.data.to_string());
                        let _ = tx.send(TuiEvent::Output {
                            role: "Tool".into(),
                            message: format!("{} ok\n{}", result.tool_name, rendered),
                        });
                    } else {
                        let _ = tx.send(TuiEvent::Output {
                            role: "Tool".into(),
                            message: format!("{} failed: {}", result.tool_name, result.error),
                        });
                    }
                }
                Ok(Err(_)) => {
                    let _ = tx.send(TuiEvent::Output {
                        role: "Error".into(),
                        message: "tool result channel closed".into(),
                    });
                }
                Err(_) => {
                    let _ = tx.send(TuiEvent::Output {
                        role: "Error".into(),
                        message: "tool request timed out".into(),
                    });
                }
            }
        });
    }

    fn build_agent_prompt(&self, user_input: &str) -> String {
        let mut prompt = String::new();
        if let Some(client_id) = &self.selected_client {
            if let Ok(mgr) = self.manager.lock() {
                if let Some(metadata) = mgr.get_client_metadata(client_id) {
                    prompt.push_str("Client Info:\n");
                    prompt.push_str(&format!("session_id: {}\n", metadata.session_id));
                    prompt.push_str(&format!("install_id: {}\n", metadata.install_id));
                    prompt.push_str(&format!("build_uuid: {}\n", metadata.build_uuid));
                    prompt.push_str(&format!("hostname: {}\n", metadata.hostname));
                    prompt.push_str(&format!("username: {}\n", metadata.username));
                    prompt.push_str(&format!("pid: {}\n", metadata.pid));
                    prompt.push_str(&format!("os: {}\n", metadata.os));
                    prompt.push_str(&format!("arch: {}\n", metadata.arch));
                    prompt.push_str(&format!("ip: {}\n", metadata.ip));
                    prompt.push_str(&format!("timestamp: {}\n", metadata.registered_at));
                    if let Ok(db) = self.db.lock() {
                        if let Ok(history) = db.read_recent_chat(&metadata.install_id, 20) {
                            if !history.is_empty() {
                                prompt.push_str("Recent Chat History:\n");
                                for (role, content) in history {
                                    prompt.push_str(&format!("{}: {}\n", role, content));
                                }
                                prompt.push('\n');
                            }
                        }
                    }
                    prompt.push('\n');
                }
                let exec_history = mgr.get_exec_history(client_id);
                if !exec_history.is_empty() {
                    prompt.push_str("Exec History:\n");
                    for entry in exec_history {
                        prompt.push_str(&format!("command: {}\n", entry.command));
                        prompt.push_str(&format!("exit_code: {}\n", entry.exit_code));
                        prompt.push_str(&format!("duration_ms: {}\n", entry.duration_ms));
                        prompt.push_str(&format!("cwd: {}\n", entry.cwd));
                        if !entry.stdout.is_empty() {
                            prompt.push_str(&format!("stdout: {}\n", entry.stdout));
                        }
                        if !entry.stderr.is_empty() {
                            prompt.push_str(&format!("stderr: {}\n", entry.stderr));
                        }
                        if !entry.env.is_empty() {
                            let env_str = entry
                                .env
                                .iter()
                                .map(|(k, v)| format!("{}={}", k, v))
                                .collect::<Vec<_>>()
                                .join("; ");
                            prompt.push_str(&format!("env: {}\n", env_str));
                        }
                        prompt.push('\n');
                    }
                }
            }
        }
        prompt.push_str(&self.context.as_prompt());
        prompt.push_str(&format!("User: {}", user_input));
        prompt
    }

    fn append_manual_tool_history(
        &self,
        client_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) {
        let install_id = self
            .manager
            .lock()
            .ok()
            .and_then(|mgr| mgr.get_client_metadata(client_id))
            .map(|m| m.install_id);
        if let Some(install_id) = install_id {
            if let Ok(db) = self.db.lock() {
                let _ = db.insert_chat(
                    &install_id,
                    "user",
                    &format!("manual_tool {} {}", tool_name, args),
                    unix_timestamp(),
                );
            }
        }
    }

    pub fn select_session_by_index(&mut self) {
        if let Some(client) = self.sessions_cache.get(self.sessions_selected) {
            let install_id = client.install_id.clone();
            let session_id = client.session_id.clone();
            self.selected_client = Some(session_id);
            self.agent_mode = true;
            self.context.clear();
            self.agent_generation += 1;
            self.push_output("Info", &format!("Selected client: {}", install_id));
        }
    }

    /// Generate tab-completion candidates based on current input buffer.
    pub fn get_completions(&self, input: &str) -> Vec<String> {
        const COMMANDS: &[&str] = &[
            "/sessions",
            "/use ",
            "/info",
            "/clean",
            "/exec ",
            "/tool ",
            "/read ",
            "/write ",
            "/upload ",
            "/download ",
            "/agent ",
            "/clear",
            "/back",
            "/help",
        ];

        // Complete `/use <install_id>`
        if let Some(rest) = input.strip_prefix("/use ") {
            let prefix = rest.trim();
            return self
                .sessions_cache
                .iter()
                .filter(|c| c.install_id.starts_with(prefix))
                .map(|c| format!("/use {}", c.install_id))
                .collect();
        }

        // Complete command names
        if input.starts_with('/') {
            return COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(input))
                .map(|cmd| cmd.to_string())
                .collect();
        }

        Vec::new()
    }

    /// Handle tab-completion: if already completing, cycle; otherwise start.
    pub fn handle_tab_completion(&mut self) {
        if self.input.is_completing() {
            self.input.next_completion();
        } else {
            let buf = self.input.buffer().to_string();
            let candidates = self.get_completions(&buf);
            self.input.set_candidates(candidates);
        }
    }

    /// Returns filtered command hints based on current input.
    /// Hides popup when input is an exact match of a command (user already completed it).
    pub fn get_popup_hints(&self) -> Vec<&'static CommandHint> {
        let buf = self.input.buffer();
        if !buf.starts_with('/') {
            return Vec::new();
        }
        // If user already typed a space after the command, don't show popup
        if buf.contains(' ') {
            return Vec::new();
        }
        COMMAND_HINTS
            .iter()
            .filter(|h| h.command.starts_with(buf) && h.command != buf)
            .collect()
    }

    /// Whether the command popup should be visible.
    pub fn popup_visible(&self) -> bool {
        self.focus == PanelFocus::Input && !self.get_popup_hints().is_empty()
    }

    /// Move popup selection up.
    pub fn popup_up(&mut self) {
        if self.popup_selected > 0 {
            self.popup_selected -= 1;
        }
    }

    /// Move popup selection down.
    pub fn popup_down(&mut self) {
        let hints = self.get_popup_hints();
        if !hints.is_empty() && self.popup_selected + 1 < hints.len() {
            self.popup_selected += 1;
        }
    }

    /// Accept the currently selected popup item into the input buffer.
    pub fn popup_accept(&mut self) {
        let hints = self.get_popup_hints();
        if let Some(hint) = hints.get(self.popup_selected) {
            // For commands that take arguments, append a space
            let needs_arg = matches!(
                hint.command,
                "/use" | "/exec" | "/tool" | "/read" | "/write" | "/upload" | "/download" | "/agent"
            );
            let text = if needs_arg {
                format!("{} ", hint.command)
            } else {
                hint.command.to_string()
            };
            self.input.set_buffer(&text);
        }
    }

    /// Reset popup selection (called when input changes).
    pub fn reset_popup(&mut self) {
        self.popup_selected = 0;
    }
}

fn save_chat(
    db: &Arc<Mutex<Db>>,
    manager: &Arc<Mutex<ClientManager>>,
    client_id: &Option<String>,
    user_input: &str,
    answer: &str,
) {
    if let Some(client_id) = client_id {
        if let Some(install_id) = manager
            .lock()
            .ok()
            .and_then(|mgr| mgr.get_client_metadata(client_id))
            .map(|m| m.install_id)
        {
            if let Ok(db) = db.lock() {
                let _ = db.insert_chat(&install_id, "user", user_input, unix_timestamp());
                let _ = db.insert_chat(&install_id, "assistant", answer, unix_timestamp());
            }
        }
    }
}

fn now_time() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
