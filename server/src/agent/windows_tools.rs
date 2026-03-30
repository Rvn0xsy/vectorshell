use crate::agent::exec_tool::ToolEventEmitter;
use crate::client_manager::ClientManager;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct PowerShellClrTool {
    manager: Arc<Mutex<ClientManager>>,
    session_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
    event_emitter: Option<ToolEventEmitter>,
}

#[derive(Clone)]
pub struct DotnetAssemblyTool {
    manager: Arc<Mutex<ClientManager>>,
    session_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
    event_emitter: Option<ToolEventEmitter>,
}

impl PowerShellClrTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        session_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
        event_emitter: Option<ToolEventEmitter>,
    ) -> Self {
        Self {
            manager,
            session_id,
            last_output,
            event_emitter,
        }
    }
}

impl DotnetAssemblyTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        session_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
        event_emitter: Option<ToolEventEmitter>,
    ) -> Self {
        Self {
            manager,
            session_id,
            last_output,
            event_emitter,
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PowerShellClrArgs {
    script: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct DotnetAssemblyArgs {
    content_base64: Option<String>,
    artifact_id: Option<String>,
    runtime_version: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    domain: Option<String>,
    #[serde(default)]
    patch_exit: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("windows tool error: {0}")]
pub struct WindowsToolError(String);

impl Tool for PowerShellClrTool {
    const NAME: &'static str = "powershell_clr";

    type Error = WindowsToolError;
    type Args = PowerShellClrArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "powershell_clr".to_string(),
            description: "Execute PowerShell via CLR host on Windows client and return output text.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "PowerShell script to execute on Windows client"
                    }
                },
                "required": ["script"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        call_remote_windows_tool(
            Arc::clone(&self.manager),
            &self.session_id,
            "powershell_clr",
            serde_json::to_value(args).map_err(|e| WindowsToolError(e.to_string()))?,
            Arc::clone(&self.last_output),
            self.event_emitter.clone(),
        )
        .await
    }
}

impl Tool for DotnetAssemblyTool {
    const NAME: &'static str = "dotnet_assembly";

    type Error = WindowsToolError;
    type Args = DotnetAssemblyArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "dotnet_assembly".to_string(),
            description: "Run a .NET EXE assembly (base64 bytes) in-memory via CLR host on Windows client.".to_string(),
            parameters: json!({
                "type": "object",
                "oneOf": [
                  {"required": ["content_base64"]},
                  {"required": ["artifact_id"]}
                ],
                "properties": {
                    "content_base64": {
                        "type": "string",
                        "description": "Base64 encoded bytes of .NET EXE assembly"
                    },
                    "artifact_id": {
                        "type": "string",
                        "description": "Server artifact id containing .NET EXE bytes"
                    },
                    "runtime_version": {
                        "type": "string",
                        "description": "CLR runtime version: v2/v3/v4 (default v4)"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed to assembly Main"
                    },
                    "domain": {
                        "type": "string",
                        "description": "Optional CLR AppDomain name"
                    },
                    "patch_exit": {
                        "type": "boolean",
                        "description": "Patch Environment.Exit to avoid killing host process"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content_base64 = if let Some(b64) = args.content_base64.clone() {
            b64
        } else if let Some(artifact_id) = args.artifact_id.as_deref() {
            let path = artifact_path(artifact_id);
            let bytes = std::fs::read(&path)
                .map_err(|e| WindowsToolError(format!("read artifact failed: {e}")))?;
            BASE64.encode(bytes)
        } else {
            return Err(WindowsToolError(
                "dotnet_assembly requires content_base64 or artifact_id".to_string(),
            ));
        };

        call_remote_windows_tool(
            Arc::clone(&self.manager),
            &self.session_id,
            "dotnet_assembly",
            json!({
                "content_base64": content_base64,
                "runtime_version": args.runtime_version,
                "args": args.args,
                "domain": args.domain,
                "patch_exit": args.patch_exit,
            }),
            Arc::clone(&self.last_output),
            self.event_emitter.clone(),
        )
        .await
    }
}

fn artifact_path(artifact_id: &str) -> PathBuf {
    PathBuf::from("data/artifacts").join(format!("{}.bin", artifact_id))
}

async fn call_remote_windows_tool(
    manager: Arc<Mutex<ClientManager>>,
    session_id: &str,
    tool_name: &str,
    args: Value,
    last_output: Arc<Mutex<Option<Value>>>,
    event_emitter: Option<ToolEventEmitter>,
) -> Result<Value, WindowsToolError> {
    let safe_args = sanitize_tool_value(&args);
    let receiver = {
        let mut mgr = manager
            .lock()
            .map_err(|_| WindowsToolError("client manager lock failed".to_string()))?;
        let (request_id, receiver) = mgr
            .dispatch_tool_call(session_id, tool_name, args.clone(), Some(120_000))
            .map_err(WindowsToolError)?;

        if let Some(emitter) = &event_emitter {
            emitter(json!({
                "event": "tool.started",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "request_id": request_id,
                "tool_name": tool_name,
                "args": safe_args.clone(),
            }));
        }
        receiver
    };

    let result = tokio::time::timeout(Duration::from_secs(120), receiver)
        .await
        .map_err(|_| WindowsToolError(format!("{tool_name} timed out")))?
        .map_err(|_| WindowsToolError(format!("{tool_name} result channel closed")))?;

    if let Some(emitter) = &event_emitter {
        emitter(json!({
            "event": "tool.finished",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "tool_name": tool_name,
            "ok": result.ok,
            "duration_ms": result.duration_ms,
            "error": result.error,
            "data": result.data,
        }));
    }

    if !result.ok {
        return Err(WindowsToolError(format!("{tool_name} failed: {}", result.error)));
    }

    let capture = json!({
        "tool": tool_name,
        "args": safe_args,
        "result": result.data,
    });
    if let Ok(mut guard) = last_output.lock() {
        *guard = Some(capture.clone());
    }
    Ok(capture)
}

fn sanitize_tool_value(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            if s.len() > 240 {
                Value::String(format!("{}...[truncated:{}]", &s[..240], s.len()))
            } else {
                Value::String(s.clone())
            }
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_tool_value).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if k == "content_base64" {
                    let len = v.as_str().map(|s| s.len()).unwrap_or(0);
                    out.insert(k.clone(), Value::String(format!("<base64:{} chars>", len)));
                } else {
                    out.insert(k.clone(), sanitize_tool_value(v));
                }
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}
