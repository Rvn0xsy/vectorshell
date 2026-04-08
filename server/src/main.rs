mod agent;
mod api;
mod builder;
mod client_manager;
mod config;
mod db;
mod event_bus;
mod mcp;
mod tui;
mod ui;

use crate::agent::Agent;
use crate::api::{ApiState, run_api_server};
use crate::builder::generate_client_binary;
use crate::client_manager::ClientManager;
use crate::config::ServerConfig;
use crate::db::Db;
use crate::event_bus::new_event_bus;
use crate::ui::UiState;
use std::sync::{Arc, Mutex};
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

    let mcp_state = config.mcp.as_ref().and_then(|mcp_cfg| {
        if mcp_cfg.enabled {
            Some(crate::mcp::McpState {
                config: mcp_cfg.clone(),
                sessions: Arc::new(crate::mcp::McpSessionStore::new()),
                manager: Arc::clone(&manager),
            })
        } else {
            None
        }
    });

    let api_state = ApiState {
        manager: Arc::clone(&manager),
        db: Arc::clone(&db),
        agent: Arc::clone(&agent),
        config: config.clone(),
        api_auth_token: config.auth.api_token.clone(),
        client_auth_token: config.auth.client_token.clone(),
        events: events.clone(),
        ui_state: Arc::clone(&ui_state),
        mcp_state,
    };

    let listen_addr = config.server.listen.clone();
    let tls_for_server = config.tls.clone();

    let server_handle = tokio::spawn(async move {
        if let Err(error) = run_api_server(listen_addr, api_state, tls_for_server).await {
            tracing::error!(%error, "server failed");
        }
    });

    tui::run_tui(manager, db, Arc::as_ref(&agent).clone(), ui_state).await;

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
