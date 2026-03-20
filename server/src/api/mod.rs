use crate::agent::Agent;
use crate::agent::exec_tool::ToolEventEmitter;
use crate::builder::generate_client_binary;
use crate::client_manager::{ClientManager, ClientMetadata, ExecHistoryEntry};
use crate::config::{ServerConfig, TlsSection};
use crate::db::Db;
use crate::event_bus::{EventBus, emit as emit_bus, ensure_channel};
use crate::ui::{UiState, ui_print};
use axum_server::tls_rustls::RustlsConfig;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::protocol::{ClientToServerMessage, ServerToClientMessage};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub manager: Arc<Mutex<ClientManager>>,
    pub db: Arc<Mutex<Db>>,
    pub agent: Arc<Agent>,
    pub config: ServerConfig,
    pub auth_token: String,
    pub events: EventBus,
    pub ui_state: Arc<Mutex<UiState>>,
    pub conversations: Arc<Mutex<HashMap<String, String>>>, // conversation_id -> connection_id
}

pub async fn run_api_server(
    listen_addr: String,
    state: ApiState,
    tls_config: Option<TlsSection>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_path = state.config.server.ws_path.clone();
    let app = Router::new()
        .route(ws_path.as_str(), get(websocket_upgrade))
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:connection_id", get(get_session))
        .route("/api/sessions/:connection_id/history", get(get_session_history))
        .route("/api/sessions/:connection_id/clean", post(clean_session))
        .route("/api/sessions/:connection_id/events", get(session_events))
        .route("/api/sessions/:connection_id/tools", post(call_tool))
        .route("/api/conversations", post(create_conversation))
        .route(
            "/api/conversations/:conversation_id/messages",
            post(conversation_message),
        )
        .route(
            "/api/conversations/:conversation_id/events",
            get(conversation_events),
        )
        .route("/api/artifacts", post(upload_artifact))
        .route("/api/artifacts/:artifact_id", get(get_artifact).delete(delete_artifact))
        .route(
            "/api/artifacts/:artifact_id/download",
            get(download_artifact),
        )
        .route("/api/clients/generate", post(generate_client))
        .route("/api/clients/download", get(download_generated_client))
        .nest_service("/webapp", ServeDir::new("webapp/dist"))
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .with_state(state);

    if let Some(tls) = tls_config {
        if tls.enabled {
            tracing::info!("api listening on {} with TLS", listen_addr);
            let rustls_config =
                RustlsConfig::from_pem_file(tls.cert_path.clone(), tls.key_path.clone()).await?;
            let addr = listen_addr.parse()?;
            axum_server::bind_rustls(addr, rustls_config)
                .serve(app.into_make_service())
                .await?;
            return Ok(());
        }
    }

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("api listening on {}", listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    message: String,
}

fn auth(headers: &HeaderMap, state: &ApiState) -> Result<(), Response> {
    let value = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let expected = format!("Bearer {}", state.auth_token);
    if value == expected {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
                message: "invalid or missing token".to_string(),
            }),
        )
            .into_response())
    }
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(error) = handle_ws_socket(socket, state).await {
            tracing::error!(%error, "websocket connection failed");
        }
    })
}

async fn handle_ws_socket(
    mut socket: WebSocket,
    state: ApiState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerToClientMessage>();
    let mut active_connection_id: Option<String> = None;

    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                if let Some(msg) = outgoing {
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            if let Err(error) = socket.send(WsMessage::Text(json.into())).await {
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
            incoming = socket.recv() => {
                let msg = match incoming {
                    Some(Ok(msg)) => msg,
                    Some(Err(error)) => {
                        tracing::error!("websocket error: {}", error);
                        break;
                    }
                    None => break,
                };

                if let WsMessage::Text(text) = msg {
                    let parsed: ClientToServerMessage = serde_json::from_str(text.as_str())?;
                    match parsed {
                        ClientToServerMessage::Register { id: _, payload } => {
                            if payload.token != state.auth_token {
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
                            if let Ok(mut mgr) = state.manager.lock() {
                                mgr.register(payload.client_id, tx.clone(), metadata);
                            }
                            active_connection_id = Some(payload.connection_id);
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                state.manager.lock(),
                                state.db.lock(),
                            ) {
                                if let Some(meta) = mgr.get_by_connection_id(&connection_id) {
                                    let _ = db.upsert_session_online(&meta, unix_timestamp());
                                }
                            }
                        }
                        ClientToServerMessage::Heartbeat { id: _, payload } => {
                            tracing::debug!(client_id = %payload.client_id, "heartbeat");
                            if let Ok(mut mgr) = state.manager.lock() {
                                mgr.update_heartbeat(&payload.client_id, payload.timestamp);
                            }
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                state.manager.lock(),
                                state.db.lock(),
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
                                &state.ui_state,
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
                                ui_print(&state.ui_state, "Result", &format!("stdout: {}", payload.stdout));
                            }
                            if !payload.stderr.is_empty() {
                                tracing::info!("stderr: {}", payload.stderr);
                                ui_print(&state.ui_state, "Result", &format!("stderr: {}", payload.stderr));
                            }

                            if let Some(connection_id) = active_connection_id.clone() {
                                emit_bus(
                                    &state.events,
                                    &connection_id,
                                    serde_json::json!({
                                        "event": "exec.result",
                                        "conversation_id": "",
                                        "timestamp": chrono::Utc::now().to_rfc3339(),
                                        "command": payload.command,
                                        "exit_code": payload.exit_code,
                                        "duration_ms": payload.duration_ms,
                                        "stdout": payload.stdout,
                                        "stderr": payload.stderr,
                                    }),
                                );
                            }
                            if let Ok(mut mgr) = state.manager.lock() {
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
                                state.manager.lock(),
                                state.db.lock(),
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
                            if let Ok(mut mgr) = state.manager.lock() {
                                mgr.record_tool_result(&id, payload.clone());
                            }

                            if let Some(connection_id) = active_connection_id.clone() {
                                emit_bus(
                                    &state.events,
                                    &connection_id,
                                    serde_json::json!({
                                        "event": "tool.result",
                                        "conversation_id": "",
                                        "timestamp": chrono::Utc::now().to_rfc3339(),
                                        "id": id,
                                        "tool_name": payload.tool_name,
                                        "ok": payload.ok,
                                        "duration_ms": payload.duration_ms,
                                        "data": payload.data,
                                        "error": payload.error,
                                    }),
                                );
                            }
                            if let (Some(connection_id), Ok(mgr), Ok(db)) = (
                                active_connection_id.clone(),
                                state.manager.lock(),
                                state.db.lock(),
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
        if let Ok(db) = state.db.lock() {
            let _ = db.mark_offline(&connection_id, unix_timestamp());
        }
    }

    Ok(())
}

async fn list_sessions(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let sessions = state
        .manager
        .lock()
        .map(|m| m.list_clients())
        .unwrap_or_default();
    Ok(Json(json!({ "sessions": sessions })))
}

async fn get_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let found = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&connection_id));

    if let Some(meta) = found {
        Ok(Json(json!(meta)))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "session not found".to_string(),
            }),
        )
            .into_response())
    }
}

async fn get_session_history(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let meta = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&connection_id))
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    error: "not_found".to_string(),
                    message: "session not found".to_string(),
                }),
            )
                .into_response()
        })?;

    let history = state
        .db
        .lock()
        .map_err(|_| internal("db lock failed"))?
        .read_chat_history(&meta.install_id, 500)
        .map_err(|e| internal(&e.to_string()))?;

    let messages = history
        .into_iter()
        .map(|(role, content, created_at)| {
            json!({
                "role": role,
                "content": content,
                "created_at": created_at,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "connection_id": connection_id,
        "install_id": meta.install_id,
        "messages": messages,
    })))
}

async fn clean_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let meta = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&connection_id));
    let Some(meta) = meta else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "session not found".to_string(),
            }),
        )
            .into_response());
    };
    if let Ok(db) = state.db.lock() {
        let _ = db.clear_install_history(&meta.install_id);
    }
    Ok(Json(json!({ "ok": true, "message": "current history cleared" })))
}

#[derive(Debug, Deserialize)]
struct ToolCallReq {
    tool_name: String,
    args: Value,
    timeout_ms: Option<u64>,
}

async fn call_tool(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
    Json(req): Json<ToolCallReq>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let meta = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&connection_id));
    let Some(meta) = meta else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "session not found".to_string(),
            }),
        )
            .into_response());
    };

    if req.tool_name == "upload_file" {
        return handle_upload_pathref(state, meta, req).await;
    }
    if req.tool_name == "download_file" {
        return handle_download_pathref(state, meta, req).await;
    }

    let original_args = req.args.clone();
    let tool_args = resolve_pathref_args(&req.tool_name, req.args).await.map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                error: "invalid_path_ref".to_string(),
                message: e,
            }),
        )
            .into_response()
    })?;

    let dispatch_args = tool_args.clone();
    let receiver = {
        let mut mgr = state.manager.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: "internal".to_string(),
                    message: "manager lock failed".to_string(),
                }),
            )
                .into_response()
        })?;
        match mgr.dispatch_tool_call(
            &meta.client_id,
            &req.tool_name,
            dispatch_args,
            req.timeout_ms.or(Some(120_000)),
        ) {
            Ok((request_id, rx)) => {
                emit_event(
                    &state,
                    &meta.connection_id,
                    json!({
                        "event": "tool.started",
                        "conversation_id": "",
                        "timestamp": now_rfc3339(),
                        "request_id": request_id,
                        "tool_name": req.tool_name,
                    }),
                );
                rx
            }
            Err(error) => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(ErrorBody {
                        error: "capability_mismatch".to_string(),
                        message: error,
                    }),
                )
                    .into_response())
            }
        }
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(req.timeout_ms.unwrap_or(120_000)),
        receiver,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::REQUEST_TIMEOUT,
            Json(ErrorBody {
                error: "tool_timeout".to_string(),
                message: "tool request timed out".to_string(),
            }),
        )
            .into_response()
    })
    .and_then(|x| {
        x.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: "internal".to_string(),
                    message: "tool result channel closed".to_string(),
                }),
            )
                .into_response()
        })
    })?;

    if req.tool_name == "download_file" {
        if let Some(dst) = extract_dst_path_from_scope(&req.tool_name, &original_args) {
            if dst.scope == "artifact" {
                let tmp_server_path = tool_args
                    .get("dst")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorBody {
                                error: "internal".to_string(),
                                message: "download temp path missing".to_string(),
                            }),
                        )
                            .into_response()
                    })?;

                let artifact_id = save_artifact_from_file(tmp_server_path)
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorBody {
                                error: "artifact_error".to_string(),
                                message: e,
                            }),
                        )
                            .into_response()
                    })?;
                let _ = fs::remove_file(tmp_server_path);

                let payload = json!({
                    "ok": result.ok,
                    "tool_name": result.tool_name,
                    "duration_ms": result.duration_ms,
                    "data": {"artifact_id": artifact_id},
                    "error": result.error,
                });
                emit_event(
                    &state,
                    &meta.connection_id,
                    json!({
                        "event": "tool.finished",
                        "conversation_id": "",
                        "timestamp": now_rfc3339(),
                        "tool_name": req.tool_name,
                        "ok": result.ok,
                        "duration_ms": result.duration_ms,
                    }),
                );
                return Ok(Json(payload));
            }
        }
    }

    let payload = json!({
        "ok": result.ok,
        "tool_name": result.tool_name,
        "duration_ms": result.duration_ms,
        "data": result.data,
        "error": result.error,
    });
    emit_event(
        &state,
        &meta.connection_id,
        json!({
            "event": "tool.finished",
            "conversation_id": "",
            "timestamp": now_rfc3339(),
            "tool_name": req.tool_name,
            "ok": result.ok,
            "duration_ms": result.duration_ms,
        }),
    );
    Ok(Json(payload))
}

async fn handle_upload_pathref(
    state: ApiState,
    meta: crate::client_manager::ClientMetadata,
    req: ToolCallReq,
) -> Result<Json<Value>, Response> {
    let src: PathRef = serde_json::from_value(
        req.args
            .get("src")
            .cloned()
            .ok_or_else(|| bad_request("missing src"))?,
    )
    .map_err(|e| unprocessable(&format!("invalid src: {e}")))?;
    let dst: PathRef = serde_json::from_value(
        req.args
            .get("dst")
            .cloned()
            .ok_or_else(|| bad_request("missing dst"))?,
    )
    .map_err(|e| unprocessable(&format!("invalid dst: {e}")))?;

    if !(src.scope == "artifact" && dst.scope == "client") {
        return Err(unprocessable(
            "upload_file supports src.scope=artifact and dst.scope=client",
        ));
    }
    let artifact_id = src
        .artifact_id
        .ok_or_else(|| unprocessable("src.artifact_id required"))?;
    let dst_path = dst.path.ok_or_else(|| unprocessable("dst.path required"))?;

    let bytes = fs::read(artifact_path(&artifact_id)).map_err(|e| internal(&e.to_string()))?;
    let chunk_size = 256 * 1024;
    let chunks = bytes.chunks(chunk_size).collect::<Vec<_>>();
    let timeout = std::time::Duration::from_millis(req.timeout_ms.unwrap_or(120_000));

    emit_event(
        &state,
        &meta.connection_id,
        json!({"event":"tool.started","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"upload_file"}),
    );

    for (idx, chunk) in chunks.iter().enumerate() {
        let payload = json!({
            "path": dst_path,
            "content_base64": base64::engine::general_purpose::STANDARD.encode(chunk),
            "append": idx > 0,
        });
        let rx = {
            let mut mgr = state
                .manager
                .lock()
                .map_err(|_| internal("manager lock failed"))?;
            let (_, rx) = mgr
                .dispatch_tool_call(
                    &meta.client_id,
                    "upload_file",
                    payload,
                    Some(timeout.as_millis() as u64),
                )
                .map_err(|e| conflict(&e))?;
            rx
        };
        let result = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| request_timeout("tool request timed out"))
            .and_then(|x| x.map_err(|_| internal("tool result channel closed")))?;
        if !result.ok {
            return Err(conflict(&result.error));
        }
        emit_event(
            &state,
            &meta.connection_id,
            json!({"event":"tool.progress","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"upload_file","percent":(((idx+1) as f64 / chunks.len() as f64) * 100.0)}),
        );
    }

    emit_event(
        &state,
        &meta.connection_id,
        json!({"event":"tool.finished","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"upload_file","ok":true,"duration_ms":0}),
    );

    Ok(Json(json!({
        "ok": true,
        "tool_name": "upload_file",
        "duration_ms": 0,
        "data": {"dst": dst_path, "size_bytes": bytes.len(), "chunks": chunks.len()},
        "error": ""
    })))
}

async fn handle_download_pathref(
    state: ApiState,
    meta: crate::client_manager::ClientMetadata,
    req: ToolCallReq,
) -> Result<Json<Value>, Response> {
    let src: PathRef = serde_json::from_value(
        req.args
            .get("src")
            .cloned()
            .ok_or_else(|| bad_request("missing src"))?,
    )
    .map_err(|e| unprocessable(&format!("invalid src: {e}")))?;
    let dst: PathRef = serde_json::from_value(
        req.args
            .get("dst")
            .cloned()
            .ok_or_else(|| bad_request("missing dst"))?,
    )
    .map_err(|e| unprocessable(&format!("invalid dst: {e}")))?;

    if src.scope != "client" {
        return Err(unprocessable("download_file requires src.scope=client"));
    }
    if !(dst.scope == "artifact" || dst.scope == "server") {
        return Err(unprocessable(
            "download_file supports dst.scope=artifact|server",
        ));
    }

    let src_path = src.path.ok_or_else(|| unprocessable("src.path required"))?;
    let timeout = std::time::Duration::from_millis(req.timeout_ms.unwrap_or(120_000));
    let mut offset = 0usize;
    let mut all = Vec::new();

    emit_event(
        &state,
        &meta.connection_id,
        json!({"event":"tool.started","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"download_file"}),
    );

    loop {
        let payload = json!({"path": src_path, "offset": offset, "limit": 256 * 1024});
        let rx = {
            let mut mgr = state
                .manager
                .lock()
                .map_err(|_| internal("manager lock failed"))?;
            let (_, rx) = mgr
                .dispatch_tool_call(
                    &meta.client_id,
                    "download_file_chunk",
                    payload,
                    Some(timeout.as_millis() as u64),
                )
                .map_err(|e| conflict(&e))?;
            rx
        };

        let result = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| request_timeout("tool request timed out"))
            .and_then(|x| x.map_err(|_| internal("tool result channel closed")))?;
        if !result.ok {
            return Err(conflict(&result.error));
        }

        let content_base64 = result
            .data
            .get("content_base64")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chunk = base64::engine::general_purpose::STANDARD
            .decode(content_base64.as_bytes())
            .map_err(|e| internal(&e.to_string()))?;
        let bytes_read = result
            .data
            .get("bytes_read")
            .and_then(Value::as_u64)
            .unwrap_or(chunk.len() as u64) as usize;
        let eof = result
            .data
            .get("eof")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        if bytes_read == 0 {
            break;
        }
        all.extend_from_slice(&chunk[..bytes_read.min(chunk.len())]);
        offset += bytes_read;

        emit_event(
            &state,
            &meta.connection_id,
            json!({"event":"tool.progress","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"download_file","detail":format!("bytes={}", offset)}),
        );

        if eof {
            break;
        }
    }

    if dst.scope == "artifact" {
        let artifact_id = save_artifact_raw_bytes(&all).map_err(|e| internal(&e))?;
        emit_event(
            &state,
            &meta.connection_id,
            json!({"event":"tool.finished","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"download_file","ok":true,"duration_ms":0}),
        );
        return Ok(Json(json!({
            "ok": true,
            "tool_name": "download_file",
            "duration_ms": 0,
            "data": {"artifact_id": artifact_id, "size_bytes": all.len()},
            "error": ""
        })));
    }

    let dst_path = dst.path.ok_or_else(|| unprocessable("dst.path required"))?;
    if let Some(parent) = std::path::Path::new(&dst_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&dst_path, &all).map_err(|e| internal(&e.to_string()))?;

    emit_event(
        &state,
        &meta.connection_id,
        json!({"event":"tool.finished","conversation_id":"","timestamp":now_rfc3339(),"tool_name":"download_file","ok":true,"duration_ms":0}),
    );
    Ok(Json(json!({
        "ok": true,
        "tool_name": "download_file",
        "duration_ms": 0,
        "data": {"dst": dst_path, "size_bytes": all.len()},
        "error": ""
    })))
}

#[derive(Debug, Deserialize)]
struct CreateConversationReq {
    connection_id: String,
    title: Option<String>,
}

async fn create_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<CreateConversationReq>,
) -> Result<(StatusCode, Json<Value>), Response> {
    auth(&headers, &state)?;
    let exists = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&req.connection_id))
        .is_some();
    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "session not found".to_string(),
            }),
        )
            .into_response());
    }

    let conversation_id = format!("conv_{}", Uuid::new_v4().simple());
    if let Ok(mut map) = state.conversations.lock() {
        map.insert(conversation_id.clone(), req.connection_id.clone());
    }

    ensure_event_channel(&state, &conversation_id);

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "conversation_id": conversation_id,
            "connection_id": req.connection_id,
            "title": req.title.unwrap_or_else(|| "conversation".to_string()),
            "created_at": now_rfc3339(),
        })),
    ))
}

#[derive(Debug, Deserialize)]
struct ConversationMessageReq {
    message: String,
}

async fn conversation_message(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(req): Json<ConversationMessageReq>,
) -> Result<(StatusCode, Json<Value>), Response> {
    auth(&headers, &state)?;
    let connection_id = state
        .conversations
        .lock()
        .ok()
        .and_then(|m| m.get(&conversation_id).cloned())
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    error: "not_found".to_string(),
                    message: "conversation not found".to_string(),
                }),
            )
                .into_response()
        })?;

    let client_id = state
        .manager
        .lock()
        .ok()
        .and_then(|m| m.get_by_connection_id(&connection_id))
        .map(|m| m.client_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorBody {
                    error: "not_found".to_string(),
                    message: "session not found".to_string(),
                }),
            )
                .into_response()
        })?;

    let manager = Arc::clone(&state.manager);
    let agent = Arc::clone(&state.agent);
    let events_state = state.clone();
    let conv = conversation_id.clone();
    let user_message = req.message.clone();

    tokio::spawn(async move {
        let conv_for_tools = conv.clone();
        let event_state_for_tools = events_state.clone();
        let emitter: ToolEventEmitter = Arc::new(move |mut payload: Value| {
            payload["conversation_id"] = Value::String(conv_for_tools.clone());
            emit_event(&event_state_for_tools, &conv_for_tools, payload);
        });

        emit_event(
            &events_state,
            &conv,
            json!({
                "event": "conversation.started",
                "conversation_id": conv,
                "timestamp": now_rfc3339(),
                "connection_id": connection_id,
            }),
        );

        match agent
            .respond_with_tools(
                &user_message,
                manager,
                &client_id,
                Arc::new(Mutex::new(crate::ui::UiState::default())),
                Some(emitter),
            )
            .await
        {
            Ok(answer) => {
                emit_event(
                    &events_state,
                    &conv,
                    json!({
                        "event": "agent.message",
                        "conversation_id": conv,
                        "timestamp": now_rfc3339(),
                        "role": "assistant",
                        "content": answer,
                        "final": true,
                    }),
                );
                emit_event(
                    &events_state,
                    &conv,
                    json!({
                        "event": "conversation.finished",
                        "conversation_id": conv,
                        "timestamp": now_rfc3339(),
                        "ok": true,
                    }),
                );
            }
            Err(error) => {
                emit_event(
                    &events_state,
                    &conv,
                    json!({
                        "event": "error",
                        "conversation_id": conv,
                        "timestamp": now_rfc3339(),
                        "code": "agent_error",
                        "message": error.to_string(),
                    }),
                );
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "accepted": true,
            "message_id": format!("msg_{}", Uuid::new_v4().simple())
        })),
    ))
}

#[derive(Debug, Deserialize)]
struct SseQuery {
    token: Option<String>,
}

async fn conversation_events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Query(query): Query<SseQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, Response> {
    let authorized = if headers.contains_key("authorization") {
        auth(&headers, &state).is_ok()
    } else {
        query.token.as_deref() == Some(state.auth_token.as_str())
    };
    if !authorized {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
                message: "invalid or missing token".to_string(),
            }),
        )
            .into_response());
    }

    let tx = ensure_event_channel(&state, &conversation_id);
    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(payload) => Some(Ok(Event::default().data(payload.to_string()))),
        Err(BroadcastStreamRecvError::Lagged(_)) => None,
    });

    Ok(Sse::new(stream))
}

async fn session_events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
    Query(query): Query<SseQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, Response> {
    let authorized = if headers.contains_key("authorization") {
        auth(&headers, &state).is_ok()
    } else {
        query.token.as_deref() == Some(state.auth_token.as_str())
    };
    if !authorized {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
                message: "invalid or missing token".to_string(),
            }),
        )
            .into_response());
    }

    let tx = ensure_event_channel(&state, &connection_id);
    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(payload) => Some(Ok(Event::default().data(payload.to_string()))),
        Err(BroadcastStreamRecvError::Lagged(_)) => None,
    });
    Ok(Sse::new(stream))
}

async fn upload_artifact(
    State(state): State<ApiState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), Response> {
    auth(&headers, &state)?;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "bad_request".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response()
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            filename = field.file_name().map(|s| s.to_string());
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorBody {
                        error: "bad_request".to_string(),
                        message: e.to_string(),
                    }),
                )
                    .into_response()
            })?;
            file_bytes = Some(data.to_vec());
        }
    }

    let bytes = file_bytes.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "bad_request".to_string(),
                message: "missing file part".to_string(),
            }),
        )
            .into_response()
    })?;

    let artifact_id = format!("art_{}", Uuid::new_v4().simple());
    let dir = PathBuf::from("data/artifacts");
    fs::create_dir_all(&dir).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response()
    })?;
    let path = dir.join(format!("{}.bin", artifact_id));
    fs::write(&path, &bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response()
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = hex::encode(hasher.finalize());

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "artifact_id": artifact_id,
            "filename": filename.unwrap_or_else(|| "upload.bin".to_string()),
            "size_bytes": bytes.len(),
            "sha256": sha,
            "expires_at": null,
        })),
    ))
}

async fn get_artifact(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(artifact_id): Path<String>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let path = artifact_path(&artifact_id);
    let meta = fs::metadata(&path).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "artifact not found".to_string(),
            }),
        )
            .into_response()
    })?;

    Ok(Json(json!({
        "artifact_id": artifact_id,
        "filename": format!("{}.bin", artifact_id),
        "size_bytes": meta.len(),
        "sha256": null,
        "expires_at": null,
    })))
}

async fn delete_artifact(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(artifact_id): Path<String>,
) -> Result<StatusCode, Response> {
    auth(&headers, &state)?;
    let path = artifact_path(&artifact_id);
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "artifact not found".to_string(),
            }),
        )
            .into_response());
    }
    fs::remove_file(path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response()
    })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn download_artifact(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(artifact_id): Path<String>,
    Query(query): Query<SseQuery>,
) -> Result<Response, Response> {
    let authorized = if headers.contains_key("authorization") {
        auth(&headers, &state).is_ok()
    } else {
        query.token.as_deref() == Some(state.auth_token.as_str())
    };
    if !authorized {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
                message: "invalid or missing token".to_string(),
            }),
        )
            .into_response());
    }
    let path = artifact_path(&artifact_id);
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "artifact not found".to_string(),
            }),
        )
            .into_response());
    }
    let bytes = fs::read(path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response()
    })?;

    Ok((
        StatusCode::OK,
        [("content-type", "application/octet-stream")],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct GenerateClientReq {
    target: Option<String>,
}

async fn generate_client(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<GenerateClientReq>,
) -> Result<Json<Value>, Response> {
    auth(&headers, &state)?;
    let normalized_target = req
        .target
        .as_deref()
        .map(normalize_target)
        .transpose()
        .map_err(|e| unprocessable(&e))?;
    let target = normalized_target.as_deref();
    generate_client_binary(&state.config, target).map_err(|e| internal(&e.to_string()))?;
    let file = generated_client_file_name(target);
    Ok(Json(json!({
        "ok": true,
        "target": target.unwrap_or("default"),
        "file": file,
        "download_url": format!("/api/clients/download?target={}", req.target.as_deref().unwrap_or("")),
    })))
}

#[derive(Debug, Deserialize)]
struct DownloadClientQuery {
    token: Option<String>,
    target: Option<String>,
}

async fn download_generated_client(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<DownloadClientQuery>,
) -> Result<Response, Response> {
    let authorized = if headers.contains_key("authorization") {
        auth(&headers, &state).is_ok()
    } else {
        query.token.as_deref() == Some(state.auth_token.as_str())
    };
    if !authorized {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
                message: "invalid or missing token".to_string(),
            }),
        )
            .into_response());
    }

    let file = generated_client_file_name(query.target.as_deref());
    let path = PathBuf::from("build/clients").join(&file);
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not_found".to_string(),
                message: "generated client not found".to_string(),
            }),
        )
            .into_response());
    }

    let bytes = fs::read(path).map_err(|e| internal(&e.to_string()))?;
    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/octet-stream"),
            ("content-disposition", &format!("attachment; filename=\"{}\"", file)),
        ],
        bytes,
    )
        .into_response())
}

fn generated_client_file_name(target: Option<&str>) -> String {
    let t = target.unwrap_or("").to_lowercase();
    if t.contains("windows") || t.contains("pc-windows") || t == "windows" {
        "vectorshell-client.exe".to_string()
    } else {
        "vectorshell-client".to_string()
    }
}

fn normalize_target(value: &str) -> Result<String, String> {
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

    if value.split('-').count() >= 3 {
        return Ok(value.to_string());
    }

    Err(format!(
        "unknown target '{}'. Use one of: linux-amd64, linux-arm64, windows-amd64, windows-arm64, macos-amd64, macos-arm64",
        value
    ))
}

#[derive(Debug, Deserialize, Clone)]
struct PathRef {
    scope: String,
    path: Option<String>,
    artifact_id: Option<String>,
}

fn extract_dst_path_from_scope(tool_name: &str, args: &Value) -> Option<PathRef> {
    if tool_name != "download_file" {
        return None;
    }
    let dst = args.get("dst")?.clone();
    serde_json::from_value(dst).ok()
}

async fn resolve_pathref_args(tool_name: &str, args: Value) -> Result<Value, String> {
    if tool_name == "upload_file" {
        let src = args
            .get("src")
            .cloned()
            .ok_or_else(|| "missing src".to_string())?;
        let dst = args
            .get("dst")
            .cloned()
            .ok_or_else(|| "missing dst".to_string())?;
        let src_ref: PathRef = serde_json::from_value(src).map_err(|e| e.to_string())?;
        let dst_ref: PathRef = serde_json::from_value(dst).map_err(|e| e.to_string())?;
        if src_ref.scope == "artifact" && dst_ref.scope == "client" {
            let artifact_id = src_ref
                .artifact_id
                .ok_or_else(|| "src.artifact_id required".to_string())?;
            let dst_path = dst_ref.path.ok_or_else(|| "dst.path required".to_string())?;
            let path = artifact_path(&artifact_id);
            let bytes = fs::read(&path).map_err(|e| format!("read artifact failed: {e}"))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Ok(json!({
                "src": artifact_id,
                "dst": dst_path,
                "path": dst_path,
                "content_base64": b64,
            }));
        }
        return Err("upload_file currently supports src=artifact, dst=client".to_string());
    }

    if tool_name == "download_file" {
        let src = args
            .get("src")
            .cloned()
            .ok_or_else(|| "missing src".to_string())?;
        let dst = args
            .get("dst")
            .cloned()
            .ok_or_else(|| "missing dst".to_string())?;
        let src_ref: PathRef = serde_json::from_value(src).map_err(|e| e.to_string())?;
        let dst_ref: PathRef = serde_json::from_value(dst).map_err(|e| e.to_string())?;
        if src_ref.scope == "client" {
            let src_path = src_ref.path.ok_or_else(|| "src.path required".to_string())?;
            if dst_ref.scope == "server" {
                let dst_path = dst_ref.path.ok_or_else(|| "dst.path required".to_string())?;
                return Ok(json!({"src": src_path, "dst": dst_path}));
            }
            if dst_ref.scope == "artifact" {
                let tmp_path = format!(
                    "/tmp/vectorshell-artifact-tmp-{}.bin",
                    Uuid::new_v4().simple()
                );
                return Ok(json!({"src": src_path, "dst": tmp_path}));
            }
        }
        return Err("download_file currently supports src=client, dst=server|artifact".to_string());
    }

    Ok(args)
}

fn artifact_path(artifact_id: &str) -> PathBuf {
    PathBuf::from("data/artifacts").join(format!("{}.bin", artifact_id))
}

fn save_artifact_raw_bytes(bytes: &[u8]) -> Result<String, String> {
    let artifact_id = format!("art_{}", Uuid::new_v4().simple());
    let dir = PathBuf::from("data/artifacts");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{}.bin", artifact_id)), bytes).map_err(|e| e.to_string())?;
    Ok(artifact_id)
}

fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: "bad_request".to_string(),
            message: msg.to_string(),
        }),
    )
        .into_response()
}

fn unprocessable(msg: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ErrorBody {
            error: "invalid_path_ref".to_string(),
            message: msg.to_string(),
        }),
    )
        .into_response()
}

fn conflict(msg: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorBody {
            error: "capability_mismatch".to_string(),
            message: msg.to_string(),
        }),
    )
        .into_response()
}

fn request_timeout(msg: &str) -> Response {
    (
        StatusCode::REQUEST_TIMEOUT,
        Json(ErrorBody {
            error: "tool_timeout".to_string(),
            message: msg.to_string(),
        }),
    )
        .into_response()
}

fn internal(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: "internal".to_string(),
            message: msg.to_string(),
        }),
    )
        .into_response()
}

async fn save_artifact_from_file(path: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let artifact_id = format!("art_{}", Uuid::new_v4().simple());
    let dir = PathBuf::from("data/artifacts");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{}.bin", artifact_id)), bytes).map_err(|e| e.to_string())?;
    Ok(artifact_id)
}

fn ensure_event_channel(state: &ApiState, conversation_id: &str) -> broadcast::Sender<Value> {
    ensure_channel(&state.events, conversation_id)
}

fn emit_event(state: &ApiState, conversation_id: &str, payload: Value) {
    emit_bus(&state.events, conversation_id, payload);
}

fn now_rfc3339() -> String {
    let now = std::time::SystemTime::now();
    let datetime: chrono::DateTime<chrono::Utc> = now.into();
    datetime.to_rfc3339()
}

fn unix_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
