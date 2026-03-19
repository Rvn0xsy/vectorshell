use crate::client_manager::{ClientManager, ClientMetadata, ExecHistoryEntry};
use crate::db::Db;
use crate::ui::{ui_print, UiState};
use crate::config::ServerConfig;
use futures_util::{SinkExt, StreamExt};
use shared::protocol::{ClientToServerMessage, ServerToClientMessage};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig as RustlsConfig};
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use std::fs::File;
use std::io::BufReader;

pub async fn run_websocket_server(
    config: ServerConfig,
    manager: Arc<Mutex<ClientManager>>,
    db: Arc<Mutex<Db>>,
    ui_state: Arc<Mutex<UiState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(&config.server.listen).await?;
    tracing::info!("listening on {}", config.server.listen);

    let tls_acceptor = if let Some(tls) = &config.tls {
        if tls.enabled {
            Some(build_tls_acceptor(tls.cert_path.as_str(), tls.key_path.as_str())?)
        } else {
            None
        }
    } else {
        None
    };

    loop {
        let (stream, _) = listener.accept().await?;
        let config = config.clone();
        let manager = Arc::clone(&manager);
        let db = Arc::clone(&db);
        let ui_state = Arc::clone(&ui_state);
        let tls_acceptor = tls_acceptor.clone();

        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, config, manager, db, ui_state, tls_acceptor).await {
                tracing::error!("connection failed: {}", error);
            }
        });
    }
}


async fn handle_connection(
    stream: tokio::net::TcpStream,
    config: ServerConfig,
    manager: Arc<Mutex<ClientManager>>,
    db: Arc<Mutex<Db>>,
    ui_state: Arc<Mutex<UiState>>,
    tls_acceptor: Option<TlsAcceptor>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(acceptor) = tls_acceptor {
        let tls_stream = acceptor.accept(stream).await?;
        let ws_stream = tokio_tungstenite::accept_hdr_async(tls_stream, |req: &Request, resp: Response| {
            if req.uri().path() != config.server.ws_path {
                return Err(ErrorResponse::new(Some("invalid websocket path".to_string())));
            }
            Ok(resp)
        })
        .await?;
        handle_ws_stream(ws_stream, config, manager, db, ui_state).await?;
    } else {
        let ws_stream = accept_hdr_async(stream, |req: &Request, resp: Response| {
            if req.uri().path() != config.server.ws_path {
                return Err(ErrorResponse::new(Some("invalid websocket path".to_string())));
            }
            Ok(resp)
        })
        .await?;
        handle_ws_stream(ws_stream, config, manager, db, ui_state).await?;
    }
    Ok(())
}

async fn handle_ws_stream<S>(
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    config: ServerConfig,
    manager: Arc<Mutex<ClientManager>>,
    db: Arc<Mutex<Db>>,
    ui_state: Arc<Mutex<UiState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerToClientMessage>();

    let mut active_connection_id: Option<String> = None;

    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                if let Some(msg) = outgoing {
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            if let Err(error) = write.send(Message::Text(json)).await {
                                tracing::error!("failed to write websocket message: {}", error);
                                break;
                            }
                        }
                        Err(error) => {
                            tracing::error!("failed to serialize message: {}", error);
                        }
                    }
                } else {
                    break;
                }
            }
            incoming = read.next() => {
                let msg = match incoming {
                    Some(Ok(msg)) => msg,
                    Some(Err(error)) => {
                        tracing::error!("websocket error: {}", error);
                        break;
                    }
                    None => break,
                };

                if let Message::Text(text) = msg {
                    let parsed: ClientToServerMessage = serde_json::from_str(&text)?;
                    match parsed {
                        ClientToServerMessage::Register { id: _, payload } => {
                            if payload.token != config.auth.token {
                                tracing::warn!(client_id = %payload.client_id, "authentication failed");
                                continue;
                            }
                            tracing::info!(client_id = %payload.client_id, "client registered");
                            let metadata = ClientMetadata::new(
                                payload.client_id.clone(),
                                payload.connection_id.clone(),
                                payload.install_id,
                                payload.build_uuid,
                                payload.hostname,
                                payload.username,
                                payload.pid,
                                payload.os,
                                payload.arch,
                                payload.ip,
                                payload.timestamp,
                                payload.capabilities,
                            );
                            if let Ok(mut mgr) = manager.lock() {
                                mgr.register(payload.client_id, tx.clone(), metadata);
                            }
                            active_connection_id = Some(payload.connection_id);
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                manager.lock(),
                                db.lock(),
                            ) {
                                if let Some(meta) = mgr.get_by_connection_id(&connection_id) {
                                    let _ = db.upsert_session_online(&meta, unix_timestamp());
                                }
                            }
                        }
                        ClientToServerMessage::Heartbeat { id: _, payload } => {
                            tracing::debug!(client_id = %payload.client_id, "heartbeat");
                            if let Ok(mut mgr) = manager.lock() {
                                mgr.update_heartbeat(&payload.client_id, payload.timestamp);
                            }
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                manager.lock(),
                                db.lock(),
                            ) {
                                if let Some(meta) = mgr.get_by_connection_id(&connection_id) {
                                    let _ = db.upsert_session_online(&meta, unix_timestamp());
                                }
                            }
                        }
                        ClientToServerMessage::Result { id, payload } => {
                            let command_clone = payload.command.clone();
                            let stdout_clone = payload.stdout.clone();
                            let stderr_clone = payload.stderr.clone();
                            let cwd_clone = payload.cwd.clone();
                            let env_clone = payload.env.clone();
                            let duration_clone = payload.duration_ms;

                            tracing::info!(
                                "result {}: exit_code={} duration_ms={} cwd={}",
                                id,
                                payload.exit_code,
                                payload.duration_ms,
                                payload.cwd
                            );
                            ui_print(
                                &ui_state,
                                "Result",
                                &format!(
                                    "exit_code={} duration_ms={} cwd={} client_id={} command={}",
                                    payload.exit_code,
                                    payload.duration_ms,
                                    payload.cwd,
                                    payload.client_id,
                                    payload.command
                                ),
                            );
                            if !payload.stdout.is_empty() {
                                tracing::info!("stdout: {}", payload.stdout);
                                ui_print(&ui_state, "Result", &format!("stdout: {}", payload.stdout));
                            }
                            if !payload.stderr.is_empty() {
                                tracing::info!("stderr: {}", payload.stderr);
                                ui_print(&ui_state, "Result", &format!("stderr: {}", payload.stderr));
                            }
                            if let Ok(mut mgr) = manager.lock() {
                                mgr.record_exec_result(
                                    &payload.client_id,
                                    &id,
                                    ExecHistoryEntry {
                                        command: command_clone,
                                        stdout: stdout_clone,
                                        stderr: stderr_clone,
                                        exit_code: payload.exit_code,
                                        duration_ms: payload.duration_ms,
                                        cwd: cwd_clone,
                                        env: env_clone,
                                    },
                                );
                            }
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                manager.lock(),
                                db.lock(),
                            ) {
                                if let Some(meta) = mgr.get_by_connection_id(&connection_id) {
                                    let data_json = serde_json::to_string(&serde_json::json!({
                                        "command": payload.command,
                                        "stdout": payload.stdout,
                                        "stderr": payload.stderr,
                                        "exit_code": payload.exit_code,
                                        "cwd": payload.cwd,
                                        "env": payload.env,
                                    }))
                                    .unwrap_or_else(|_| "{}".to_string());
                                    let _ = db.insert_command_history(
                                        &meta.connection_id,
                                        &meta.install_id,
                                        "exec",
                                        true,
                                        &data_json,
                                        "",
                                        duration_clone,
                                        unix_timestamp(),
                                    );
                                }
                            }
                        }
                        ClientToServerMessage::ToolResult { id, payload } => {
                            if let Ok(mut mgr) = manager.lock() {
                                mgr.record_tool_result(&id, payload.clone());
                            }
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                manager.lock(),
                                db.lock(),
                            ) {
                                if let Some(meta) = mgr.get_by_connection_id(&connection_id) {
                                    let data_json = serde_json::to_string(&payload.data)
                                        .unwrap_or_else(|_| "null".to_string());
                                    let _ = db.insert_command_history(
                                        &meta.connection_id,
                                        &meta.install_id,
                                        &payload.tool_name,
                                        payload.ok,
                                        &data_json,
                                        &payload.error,
                                        payload.duration_ms,
                                        unix_timestamp(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(connection_id) = active_connection_id {
        if let Ok(db) = db.lock() {
            let _ = db.mark_offline(&connection_id, unix_timestamp());
        }
    }

    Ok(())
}

fn unix_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn build_tls_acceptor(
    cert_path: &str,
    key_path: &str,
) -> Result<TlsAcceptor, Box<dyn std::error::Error + Send + Sync>> {
    let cert_file = File::open(cert_path)?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = certs(&mut cert_reader)?
        .into_iter()
        .map(Certificate)
        .collect::<Vec<_>>();

    let key_file = File::open(key_path)?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys = pkcs8_private_keys(&mut key_reader)
        .map(|keys| keys.into_iter().map(PrivateKey).collect::<Vec<_>>())
        .unwrap_or_default();

    if keys.is_empty() {
        let key_file = File::open(key_path)?;
        let mut key_reader = BufReader::new(key_file);
        keys = rsa_private_keys(&mut key_reader)
            .map(|keys| keys.into_iter().map(PrivateKey).collect::<Vec<_>>())
            .unwrap_or_default();
    }

    let key = keys
        .into_iter()
        .next()
        .ok_or_else(|| "no private key found")?;

    let rustls_config = RustlsConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(TlsAcceptor::from(Arc::new(rustls_config)))
}
