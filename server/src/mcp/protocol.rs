#![allow(dead_code)]

//! MCP JSON-RPC protocol types for MCP 2025-11-25.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// MCP JSON-RPC 2.0 response (success).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
        }
    }

    pub fn error(id: Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            })),
        }
    }
}

/// MCP JSON-RPC 2.0 error response (named differently to avoid conflict).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub id: Value,
    pub error: JsonRpcErrorDetail,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcErrorDetail {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(id: Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            error: JsonRpcErrorDetail {
                code,
                message: message.into(),
                data: None,
            },
        }
    }
}

/// JSON-RPC error codes.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    pub const TOOL_NOT_FOUND: i32 = -32001;
    pub const TOOL_ERROR: i32 = -32002;
}

/// MCP protocol version.
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

/// Initialize request params.
#[derive(Debug, Deserialize)]
pub struct InitializeParams {
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub client_info: Value,
}

/// Initialize result.
#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: InitializeCapabilities,
    pub server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
pub struct InitializeCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// tools/list params (optional cursor for pagination — not used).
#[derive(Debug, Deserialize)]
pub struct ListToolsParams {
    // No required fields in basic tools/list
}

/// tools/list result.
#[derive(Debug, Serialize)]
pub struct ListToolsResult {
    pub tools: Vec<McpTool>,
}

#[derive(Debug, Serialize)]
pub struct McpTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

/// tools/call params.
#[derive(Debug, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// tools/call result.
#[derive(Debug, Serialize)]
pub struct CallToolResult {
    pub content: Vec<McpContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct McpContent {
    pub r#type: String,
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl McpContent {
    pub fn text(t: String) -> Self {
        Self {
            r#type: "text".into(),
            text: Some(t),
            data: None,
            mime_type: None,
        }
    }

    pub fn error(t: String) -> Self {
        Self {
            r#type: "text".into(),
            text: Some(t),
            data: None,
            mime_type: None,
        }
    }
}
