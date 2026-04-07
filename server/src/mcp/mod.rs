#![allow(dead_code)]

//! VectorShell MCP Server.
//!
//! Implements the MCP 2025-11-25 protocol (Streamable HTTP+SSE) as an
//! `/mcp` endpoint on the existing axum API server.

mod protocol;
mod session;
mod tools;

pub use protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use session::McpSessionStore;

use crate::client_manager::ClientManager;
use crate::config::McpSection;
use protocol::{
    CallToolParams, InitializeParams, InitializeResult, InitializeCapabilities,
    ServerInfo, MCP_PROTOCOL_VERSION,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// Shared MCP state threaded through the API.
#[derive(Clone)]
pub struct McpState {
    pub config: McpSection,
    pub sessions: Arc<McpSessionStore>,
    pub manager: Arc<Mutex<ClientManager>>,
}

impl McpState {
    /// Dispatch a tool call to the ClientManager.
    pub async fn dispatch_tool(
        &self,
        install_id: &str,
        tool_name: &str,
        args: Value,
        timeout_ms: Option<u64>,
    ) -> Result<Value, String> {
        let session_id = {
            let manager = self.manager.lock().map_err(|e| e.to_string())?;
            manager
                .get_by_install_id(install_id)
                .map(|m| m.session_id.clone())
                .ok_or_else(|| format!("no active session for install_id: {}", install_id))?
        };

        let (_request_id, receiver) = {
            let mut manager = self.manager.lock().map_err(|e| e.to_string())?;
            manager
                .dispatch_tool_call(&session_id, tool_name, args, timeout_ms)
                .map_err(|e| format!("dispatch_tool_call failed: {}", e))?
        };

        // Wait for result with timeout
        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(60_000) + 5000);
        let result = tokio::time::timeout(timeout, receiver)
            .await
            .map_err(|_| "tool call timed out".to_string())
            .map_err(|e| e)?
            .map_err(|_| "tool result channel closed".to_string())?;

        if result.ok {
            Ok(result.data)
        } else {
            Err(result.error)
        }
    }
}

/// Handle an incoming JSON-RPC request.
pub async fn handle_jsonrpc(
    state: &McpState,
    req: JsonRpcRequest,
) -> Result<Option<Value>, String> {
    match req.method.as_str() {
        "initialize" => handle_initialize(state, req).map(Some),
        "notifications/initialized" => {
            // Notification — no response
            Ok(None)
        }
        "tools/list" => handle_list_tools(state, req).map(Some),
        "tools/call" => handle_call_tool(state, req).await.map(Some),
        _ => Err(format!("method not found: {}", req.method)),
    }
}

fn handle_initialize(_state: &McpState, req: JsonRpcRequest) -> Result<Value, String> {
    let _params: InitializeParams = serde_json::from_value(req.params)
        .map_err(|e| format!("invalid initialize params: {}", e))?;

    // Mark session as initialized
    if let Some(session_id) = req.id.as_ref().and_then(|id| id.as_str().or_else(|| {
        // id could be a number
        None
    })) {
        // Session is identified by MCP-Session-Id header, not by id
        let _ = session_id;
    }
    // Sessions are actually identified by MCP-Session-Id header, handled at route level

    let result = InitializeResult {
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: InitializeCapabilities {
            tools: Some(json!({})),
        },
        server_info: ServerInfo {
            name: "vectorshell".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
    };

    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

fn handle_list_tools(_state: &McpState, _req: JsonRpcRequest) -> Result<Value, String> {
    let result = tools::list_mcp_tools();
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

async fn handle_call_tool(state: &McpState, req: JsonRpcRequest) -> Result<Value, String> {
    let params: CallToolParams = serde_json::from_value(req.params)
        .map_err(|e| format!("invalid tools/call params: {}", e))?;

    let result = tools::call_mcp_tool(&params, |install_id, tool_name, args, timeout| {
        let state = state.clone();
        let install_id = install_id.to_string();
        let tool_name = tool_name.to_string();
        Box::pin(async move { state.dispatch_tool(&install_id, &tool_name, args, timeout).await })
    })
    .await?;

    serde_json::to_value(&result).map_err(|e| e.to_string())
}

/// Build a JSON-RPC error response.
pub fn jsonrpc_error(id: Value, code: i32, message: &str) -> Value {
    serde_json::to_value(JsonRpcError::new(id, code, message)).unwrap_or_default()
}

/// Build a JSON-RPC success response.
pub fn jsonrpc_response(id: Value, result: Value) -> Value {
    serde_json::to_value(JsonRpcResponse::success(id, result)).unwrap_or_default()
}
