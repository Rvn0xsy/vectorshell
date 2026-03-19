mod agent;
mod builder;
mod client_manager;
mod config;
mod ui;
mod websocket;

use crate::agent::Agent;
use crate::builder::generate_client_binary;
use crate::client_manager::ClientManager;
use crate::config::ServerConfig;
use crate::ui::{prompt_line, set_prompt, set_waiting, ui_print, UiState};
use crate::websocket::run_websocket_server;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt};
use std::io::Write;
use tracing_appender::rolling;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let agent = Agent::new(&config);
    let manager = Arc::new(Mutex::new(ClientManager::new()));
    let ui_state = Arc::new(Mutex::new(UiState::default()));

    let manager_for_ws = Arc::clone(&manager);
    let ui_for_ws = Arc::clone(&ui_state);
    let ws_handle = tokio::spawn(async move {
        if let Err(error) = run_websocket_server(config, manager_for_ws, ui_for_ws).await {
            tracing::error!(%error, "websocket server failed");
        }
    });

    run_repl(manager, agent, ui_state).await;

    ws_handle.await?;
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
                    ui_print(&ui_state, "Info", &format!("- {client}"));
                }
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/use ") {
            selected_client = Some(rest.trim().to_string());
            agent_mode = true;
            context.clear();
            tracing::info!(client_id = %rest.trim(), "selected client");
            ui_print(&ui_state, "Info", &format!("selected client: {}", rest.trim()));
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
        if let Some(rest) = trimmed.strip_prefix("/agent ") {
            let prompt = build_agent_prompt(&manager, &selected_client, &context, rest.trim());
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
                let prompt = build_agent_prompt(&manager, &selected_client, &context, trimmed);
                let start = std::time::Instant::now();
                tracing::info!("ai.request.start type=tool");
                tracing::debug!("ai.request.prompt: {}", prompt);
                match agent
                    .respond_with_tools(&prompt, Arc::clone(&manager), &client_id, Arc::clone(&ui_state))
                    .await
                {
                    Ok(answer) => {
                        let elapsed = start.elapsed().as_millis();
                        tracing::info!("ai.request.done type=tool duration_ms={}", elapsed);
                        tracing::debug!("ai.response.text: {}", answer);
                        ui_print(&ui_state, "Agent", &answer);
                        context.push(format!("User: {}", trimmed));
                        context.push(format!("AI: {}", answer));
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
    ui_print(&ui_state, "Info", "/use <client_id>        Select a client (enters agent mode)");
    ui_print(&ui_state, "Info", "/exec <command>         Execute raw command on selected client");
    ui_print(&ui_state, "Info", "/agent <prompt>         Ask AI for a text response only");
    ui_print(&ui_state, "Info", "/clear                  Clear agent context history");
    ui_print(&ui_state, "Info", "/back                   Exit agent mode / unselect client");
    ui_print(&ui_state, "Info", "/help                   Show this help");
}

fn build_agent_prompt(
    manager: &Arc<Mutex<ClientManager>>,
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
                prompt.push_str(&format!("hostname: {}\n", metadata.hostname));
                prompt.push_str(&format!("os: {}\n", metadata.os));
                prompt.push_str(&format!("arch: {}\n", metadata.arch));
                prompt.push_str(&format!("ip: {}\n", metadata.ip));
                prompt.push_str(&format!("timestamp: {}\n", metadata.registered_at));
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
