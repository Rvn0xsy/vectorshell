mod agent;
mod api;
mod builder;
mod client_manager;
mod config;
mod db;
mod event_bus;
mod ui;

use crate::agent::Agent;
use crate::api::{ApiState, run_api_server};
use crate::builder::generate_client_binary;
use crate::client_manager::ClientManager;
use crate::config::ServerConfig;
use crate::db::Db;
use crate::event_bus::new_event_bus;
use crate::ui::{prompt_line, set_prompt, set_waiting, ui_print, UiState};
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt};
use std::io::Write;
use tracing_appender::rolling;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_logging();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter()
        .position(|arg| arg == "--config")
        .and_then(|idx| args.get(idx + 1))
        .cloned()
        .unwrap_or_else(|| "config/config.toml".to_string());

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_usage();
        return Ok(());
    }

    let config = ServerConfig::load(&config_path)?;

    if args.iter().any(|arg| arg == "generate-client") {
        let target = args
            .iter()
            .position(|arg| arg == "--target")
            .and_then(|idx| args.get(idx + 1))
            .map(|value| value.as_str());
        let target = match target {
            Some(value) => Some(parse_target(value).map_err(|err| format!("{err}"))?),
            None => None,
        };
        generate_client_binary(&config, target.as_deref())?;
        println!("client generated in build/clients");
        return Ok(());
    }
    tracing::info!("loaded config from {}", config_path);
    let agent = Arc::new(Agent::new(&config));
    let manager = Arc::new(Mutex::new(ClientManager::new()));
    let db = Arc::new(Mutex::new(Db::open("data/vectorshell.db")?));
    let events = new_event_bus();
    let ui_state = Arc::new(Mutex::new(UiState::default()));

    let api_state = ApiState {
        manager: Arc::clone(&manager),
        db: Arc::clone(&db),
        agent: Arc::clone(&agent),
        config: config.clone(),
        auth_token: config.auth.token.clone(),
        events: Arc::clone(&events),
        ui_state: Arc::clone(&ui_state),
        conversations: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    let listen_addr = config.server.listen.clone();
    let tls_for_server = config.tls.clone();

    let server_handle = tokio::spawn(async move {
        if let Err(error) = run_api_server(listen_addr, api_state, tls_for_server).await {
            tracing::error!(%error, "server failed");
        }
    });

    run_repl(manager, db, Arc::as_ref(&agent).clone(), ui_state).await;

    server_handle.await?;
    Ok(())
}

fn print_usage() {
    println!("vectorshell-server [--config <path>] [generate-client]");
    println!("  --config <path>   Path to config/config.toml");
    println!("  generate-client   Build and emit client binary (release)");
    println!("  --target <triple> Build for target triple (use with generate-client)");
    println!("    Common targets:");
    println!("      linux-amd64  -> x86_64-unknown-linux-gnu");
    println!("      linux-arm64  -> aarch64-unknown-linux-gnu");
    println!("      windows-amd64 -> x86_64-pc-windows-gnu");
    println!("      windows-arm64 -> aarch64-pc-windows-gnu");
    println!("      macos-amd64  -> x86_64-apple-darwin");
    println!("      macos-arm64  -> aarch64-apple-darwin");
    println!("  -h, --help        Show this help");
}

fn parse_target(value: &str) -> Result<String, String> {
    let normalized = value.to_lowercase();
    let mapped = match normalized.as_str() {
        "linux-amd64" | "linux-x86_64" => Some("x86_64-unknown-linux-gnu"),
        "linux-arm64" | "linux-aarch64" => Some("aarch64-unknown-linux-gnu"),
        "windows-amd64" | "windows-x86_64" => Some("x86_64-pc-windows-gnu"),
        "windows-arm64" | "windows-aarch64" => Some("aarch64-pc-windows-gnu"),
        "macos-amd64" | "macos-x86_64" => Some("x86_64-apple-darwin"),
        "macos-arm64" | "macos-aarch64" => Some("aarch64-apple-darwin"),
        "linux" => Some("x86_64-unknown-linux-gnu"),
        "windows" => Some("x86_64-pc-windows-gnu"),
        "macos" | "darwin" => Some("aarch64-apple-darwin"),
        _ => None,
    };

    if let Some(mapped) = mapped {
        return Ok(mapped.to_string());
    }

    if looks_like_triple(value) {
        return Ok(value.to_string());
    }

    Err(format!(
        "unknown target '{}'. Use one of: linux-amd64, linux-arm64, windows-amd64, windows-arm64, macos-amd64, macos-arm64",
        value
    ))
}

fn looks_like_triple(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() >= 3
}

fn init_logging() {
    let _ = std::fs::create_dir_all("logs");
    let file_appender = rolling::daily("logs", "vectorshell.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(guard);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[derive(Debug, Default)]
struct AgentContext {
    history: Vec<String>,
}

impl AgentContext {
    fn push(&mut self, entry: String) {
        self.history.push(entry);
    }

    fn clear(&mut self) {
        self.history.clear();
    }

    fn as_prompt(&self) -> String {
        if self.history.is_empty() {
            "".to_string()
        } else {
            format!("Context:\n{}\n", self.history.join("\n"))
        }
    }
}

async fn run_repl(
    manager: Arc<Mutex<ClientManager>>,
    db: Arc<Mutex<Db>>,
    agent: Agent,
    ui_state: Arc<Mutex<UiState>>,
) {
    let stdin = io::BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut selected_client: Option<String> = None;
    let mut agent_mode = false;
    let mut context = AgentContext::default();

    loop {
        let prompt = prompt_line();
        set_prompt(&ui_state, prompt.clone());
        set_waiting(&ui_state, true);
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => break,
        };
        set_waiting(&ui_state, false);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }


        if trimmed == "/help" {
            print_repl_help();
            continue;
        }

        if trimmed == "/sessions" {
            let clients = manager.lock().map(|mgr| mgr.list_clients()).unwrap_or_default();
            tracing::info!(count = clients.len(), "listed clients");
            if clients.is_empty() {
                ui_print(&ui_state, "Info", "no clients connected");
            } else {
                ui_print(&ui_state, "Info", "clients:");
                for client in clients {
                    ui_print(
                        &ui_state,
                        "Info",
                        &format!(
                            "- conn={} host={} user={} pid={} ip={} build={}",
                            client.connection_id,
                            client.hostname,
                            client.username,
                            client.pid,
                            client.ip,
                            client.build_uuid
                        ),
                    );
                }
            }
            continue;
        }

        if trimmed == "/info" {
            if let Some(client_id) = selected_client.clone() {
                if let Ok(mgr) = manager.lock() {
                    if let Some(meta) = mgr.get_client_metadata(&client_id) {
                        ui_print(
                            &ui_state,
                            "Info",
                            &format!(
                                "connection_id={} hostname={} user={} pid={} ip={} os={} arch={} build_uuid={} install_id={} last_heartbeat={}",
                                meta.connection_id,
                                meta.hostname,
                                meta.username,
                                meta.pid,
                                meta.ip,
                                meta.os,
                                meta.arch,
                                meta.build_uuid,
                                meta.install_id,
                                meta.last_heartbeat
                            ),
                        );
                    } else {
                        ui_print(&ui_state, "Error", "selected client not found");
                    }
                }
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }

        if trimmed == "/clean" {
            context.clear();
            if let Some(client_id) = selected_client.clone() {
                let install_id = manager
                    .lock()
                    .ok()
                    .and_then(|mgr| mgr.get_client_metadata(&client_id))
                    .map(|m| m.install_id)
                    .unwrap_or_default();
                if !install_id.is_empty() {
                    if let Ok(db) = db.lock() {
                        let _ = db.clear_install_history(&install_id);
                    }
                }
            }
            ui_print(&ui_state, "Info", "current history cleared");
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/use ") {
            let wanted = rest.trim();
            let mapped = manager
                .lock()
                .ok()
                .and_then(|mgr| {
                    mgr.get_by_connection_id(wanted)
                        .or_else(|| mgr.get_client_metadata(wanted))
                })
                .map(|m| m.client_id);
            if let Some(client_id) = mapped {
                selected_client = Some(client_id.clone());
                agent_mode = true;
                context.clear();
                tracing::info!(client_id = %client_id, "selected client");
                ui_print(&ui_state, "Info", &format!("selected client: {}", wanted));
            } else {
                ui_print(&ui_state, "Error", "client not found (use /sessions)");
            }
            continue;
        }

        if trimmed == "/back" {
            selected_client = None;
            agent_mode = false;
            context.clear();
            ui_print(&ui_state, "Info", "returned to top level");
            continue;
        }

        if trimmed == "/clear" {
            context.clear();
            ui_print(&ui_state, "Info", "context cleared");
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/exec ") {
            if let Some(client_id) = selected_client.clone() {
                ui_print(&ui_state, "Exec", rest.trim());
                if let Err(error) = manager.lock().map(|mgr| mgr.send_exec(&client_id, rest.trim())) {
                    ui_print(&ui_state, "Error", &format!("failed to send exec: {error:?}"));
                }
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <client_id>`");
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/tool ") {
            if let Some(client_id) = selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let tool_name = parts.next().unwrap_or("").trim();
                if tool_name.is_empty() {
                    ui_print(&ui_state, "Error", "usage: /tool <tool_name> <json_args>");
                    continue;
                }
                let args_raw = parts.next().unwrap_or("{}").trim();
                let args = match serde_json::from_str::<serde_json::Value>(args_raw) {
                    Ok(value) => value,
                    Err(error) => {
                        ui_print(&ui_state, "Error", &format!("invalid json args: {error}"));
                        continue;
                    }
                };
                context.push(format!("ManualTool: {} {}", tool_name, args));
                append_manual_tool_history(&db, &manager, &client_id, tool_name, &args);
                dispatch_tool_and_print(&manager, &ui_state, &client_id, tool_name, args).await;
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }

        if let Some(path) = trimmed.strip_prefix("/read ") {
            if let Some(client_id) = selected_client.clone() {
                let path = path.trim();
                if path.is_empty() {
                    ui_print(&ui_state, "Error", "usage: /read <path>");
                    continue;
                }
                let args = serde_json::json!({ "path": path });
                context.push(format!("ManualTool: read_file {}", args));
                append_manual_tool_history(&db, &manager, &client_id, "read_file", &args);
                dispatch_tool_and_print(&manager, &ui_state, &client_id, "read_file", args).await;
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/write ") {
            if let Some(client_id) = selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let path = parts.next().unwrap_or("").trim();
                let content = parts.next().unwrap_or("");
                if path.is_empty() {
                    ui_print(&ui_state, "Error", "usage: /write <path> <content>");
                    continue;
                }
                let args = serde_json::json!({ "path": path, "content": content });
                context.push(format!("ManualTool: write_file {}", args));
                append_manual_tool_history(&db, &manager, &client_id, "write_file", &args);
                dispatch_tool_and_print(&manager, &ui_state, &client_id, "write_file", args).await;
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/upload ") {
            if let Some(client_id) = selected_client.clone() {
                let mut parts = rest.trim().splitn(2, ' ');
                let src = parts.next().unwrap_or("").trim();
                let dst = parts.next().unwrap_or("").trim();
                if src.is_empty() || dst.is_empty() {
                    ui_print(&ui_state, "Error", "usage: /upload <src> <dst>");
                    continue;
                }
                let args = serde_json::json!({ "src": src, "dst": dst });
                context.push(format!("ManualTool: upload_file {}", args));
                append_manual_tool_history(&db, &manager, &client_id, "upload_file", &args);
                dispatch_tool_and_print(&manager, &ui_state, &client_id, "upload_file", args).await;
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }

        if let Some(path) = trimmed.strip_prefix("/download ") {
            if let Some(client_id) = selected_client.clone() {
                let mut parts = path.trim().splitn(2, ' ');
                let src = parts.next().unwrap_or("").trim();
                let dst = parts.next().unwrap_or("").trim();
                if src.is_empty() || dst.is_empty() {
                    ui_print(&ui_state, "Error", "usage: /download <src> <dst>");
                    continue;
                }
                let args = serde_json::json!({ "src": src, "dst": dst });
                context.push(format!("ManualTool: download_file {}", args));
                append_manual_tool_history(&db, &manager, &client_id, "download_file", &args);
                dispatch_tool_and_print(&manager, &ui_state, &client_id, "download_file", args).await;
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <connection_id>`");
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("/agent ") {
            let prompt = build_agent_prompt(&manager, &db, &selected_client, &context, rest.trim());
            let start = std::time::Instant::now();
            tracing::info!("ai.request.start type=text");
            tracing::debug!("ai.request.prompt: {}", prompt);
            match agent.respond_text(&prompt).await {
                Ok(answer) => {
                    let elapsed = start.elapsed().as_millis();
                    tracing::info!("ai.request.done type=text duration_ms={}", elapsed);
                    tracing::debug!("ai.response.text: {}", answer);
                    ui_print(&ui_state, "Agent", &answer);
                    context.push(format!("AI: {}", answer));
                    if let Some(client_id) = selected_client.clone() {
                        if let Some(install_id) = manager
                            .lock()
                            .ok()
                            .and_then(|mgr| mgr.get_client_metadata(&client_id))
                            .map(|m| m.install_id)
                        {
                            if let Ok(db) = db.lock() {
                                let _ = db.insert_chat(&install_id, "user", rest.trim(), unix_timestamp());
                                let _ = db.insert_chat(&install_id, "assistant", &answer, unix_timestamp());
                            }
                        }
                    }
                }
                Err(error) => {
                    let elapsed = start.elapsed().as_millis();
                    tracing::error!("ai.request.error type=text duration_ms={} error={}", elapsed, error);
                    ui_print(&ui_state, "Error", &format!("agent error: {error}"));
                }
            }
            continue;
        }

        if agent_mode {
            if let Some(client_id) = selected_client.clone() {
                let prompt = build_agent_prompt(&manager, &db, &selected_client, &context, trimmed);
                let start = std::time::Instant::now();
                tracing::info!("ai.request.start type=tool");
                tracing::debug!("ai.request.prompt: {}", prompt);
                match agent
                    .respond_with_tools(&prompt, Arc::clone(&manager), &client_id, Arc::clone(&ui_state), None)
                    .await
                {
                    Ok(answer) => {
                        let elapsed = start.elapsed().as_millis();
                        tracing::info!("ai.request.done type=tool duration_ms={}", elapsed);
                        tracing::debug!("ai.response.text: {}", answer);
                        ui_print(&ui_state, "Agent", &answer);
                        context.push(format!("User: {}", trimmed));
                        context.push(format!("AI: {}", answer));
                        if let Some(install_id) = manager
                            .lock()
                            .ok()
                            .and_then(|mgr| mgr.get_client_metadata(&client_id))
                            .map(|m| m.install_id)
                        {
                            if let Ok(db) = db.lock() {
                                let _ = db.insert_chat(&install_id, "user", trimmed, unix_timestamp());
                                let _ = db.insert_chat(&install_id, "assistant", &answer, unix_timestamp());
                            }
                        }
                    }
                    Err(error) => {
                        let elapsed = start.elapsed().as_millis();
                        tracing::error!("ai.request.error type=tool duration_ms={} error={}", elapsed, error);
                        ui_print(&ui_state, "Error", &format!("agent error: {error}"));
                    }
                }
            } else {
                ui_print(&ui_state, "Error", "no client selected. use `/use <client_id>`");
            }
            continue;
        }

        ui_print(&ui_state, "Info", "unknown command (use /help)");
    }
}

fn print_repl_help() {
    let ui_state = Arc::new(Mutex::new(UiState::default()));
    ui_print(&ui_state, "Info", "/sessions               List connected clients");
    ui_print(&ui_state, "Info", "/use <connection_id>    Select a client (enters agent mode)");
    ui_print(&ui_state, "Info", "/info                   Show selected session details");
    ui_print(&ui_state, "Info", "/clean                  Clear selected install history + context");
    ui_print(&ui_state, "Info", "/exec <command>         Execute raw command on selected client");
    ui_print(&ui_state, "Info", "/tool <name> <json>    Dispatch generic tool call to selected client");
    ui_print(&ui_state, "Info", "/read <path>            Shortcut for read_file tool");
    ui_print(&ui_state, "Info", "/write <path> <content> Shortcut for write_file tool");
    ui_print(&ui_state, "Info", "/upload <src> <dst>     Upload server-local file to client path");
    ui_print(&ui_state, "Info", "/download <src> <dst>   Download client file to server-local path");
    ui_print(&ui_state, "Info", "/agent <prompt>         Ask AI for a text response only");
    ui_print(&ui_state, "Info", "/clear                  Clear agent context history");
    ui_print(&ui_state, "Info", "/back                   Exit agent mode / unselect client");
    ui_print(&ui_state, "Info", "/help                   Show this help");
}

fn build_agent_prompt(
    manager: &Arc<Mutex<ClientManager>>,
    db: &Arc<Mutex<Db>>,
    selected_client: &Option<String>,
    context: &AgentContext,
    user_input: &str,
) -> String {
    let mut prompt = String::new();

    if let Some(client_id) = selected_client {
        if let Ok(mgr) = manager.lock() {
            if let Some(metadata) = mgr.get_client_metadata(client_id) {
                prompt.push_str("Client Info:\n");
                prompt.push_str(&format!("client_id: {}\n", metadata.client_id));
                prompt.push_str(&format!("connection_id: {}\n", metadata.connection_id));
                prompt.push_str(&format!("install_id: {}\n", metadata.install_id));
                prompt.push_str(&format!("build_uuid: {}\n", metadata.build_uuid));
                prompt.push_str(&format!("hostname: {}\n", metadata.hostname));
                prompt.push_str(&format!("username: {}\n", metadata.username));
                prompt.push_str(&format!("pid: {}\n", metadata.pid));
                prompt.push_str(&format!("os: {}\n", metadata.os));
                prompt.push_str(&format!("arch: {}\n", metadata.arch));
                prompt.push_str(&format!("ip: {}\n", metadata.ip));
                prompt.push_str(&format!("timestamp: {}\n", metadata.registered_at));
                if let Ok(db) = db.lock() {
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

            let history = mgr.get_exec_history(client_id);
            if !history.is_empty() {
                prompt.push_str("Exec History:\n");
                for entry in history {
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

    prompt.push_str(&context.as_prompt());
    prompt.push_str(&format!("User: {}", user_input));
    prompt
}

fn unix_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn dispatch_tool_and_print(
    manager: &Arc<Mutex<ClientManager>>,
    ui_state: &Arc<Mutex<UiState>>,
    client_id: &str,
    tool_name: &str,
    args: serde_json::Value,
) {
    let receiver = {
        let mut mgr = match manager.lock() {
            Ok(mgr) => mgr,
            Err(error) => {
                ui_print(ui_state, "Error", &format!("manager lock failed: {error}"));
                return;
            }
        };
        match mgr.dispatch_tool_call(client_id, tool_name, args, Some(60_000)) {
            Ok((_, rx)) => rx,
            Err(error) => {
                ui_print(ui_state, "Error", &format!("dispatch failed: {error}"));
                return;
            }
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(65), receiver).await {
        Ok(Ok(result)) => {
            if result.ok {
                let rendered = serde_json::to_string_pretty(&result.data)
                    .unwrap_or_else(|_| result.data.to_string());
                ui_print(ui_state, "Tool", &format!("{} ok\n{}", result.tool_name, rendered));
            } else {
                ui_print(
                    ui_state,
                    "Tool",
                    &format!("{} failed: {}", result.tool_name, result.error),
                );
            }
        }
        Ok(Err(_)) => {
            ui_print(ui_state, "Error", "tool result channel closed");
        }
        Err(_) => {
            ui_print(ui_state, "Error", "tool request timed out");
        }
    }
}

fn append_manual_tool_history(
    db: &Arc<Mutex<Db>>,
    manager: &Arc<Mutex<ClientManager>>,
    client_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
) {
    let install_id = manager
        .lock()
        .ok()
        .and_then(|mgr| mgr.get_client_metadata(client_id))
        .map(|m| m.install_id);
    if let Some(install_id) = install_id {
        if let Ok(db) = db.lock() {
            let _ = db.insert_chat(
                &install_id,
                "user",
                &format!("manual_tool {} {}", tool_name, args),
                unix_timestamp(),
            );
        }
    }
}
