use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crate::client_manager::ClientManager;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CHUNK_SIZE: usize = 256 * 1024;

#[derive(Clone)]
pub struct ReadFileTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
}

#[derive(Clone)]
pub struct WriteFileTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
}

#[derive(Clone)]
pub struct UploadFileTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
}

#[derive(Clone)]
pub struct DownloadFileTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
}

impl ReadFileTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
    ) -> Self {
        Self {
            manager,
            client_id,
            last_output,
        }
    }
}

impl WriteFileTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
    ) -> Self {
        Self {
            manager,
            client_id,
            last_output,
        }
    }
}

impl UploadFileTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
    ) -> Self {
        Self {
            manager,
            client_id,
            last_output,
        }
    }
}

impl DownloadFileTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
    ) -> Self {
        Self {
            manager,
            client_id,
            last_output,
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ReadFileArgs {
    path: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct UploadFileArgs {
    src: String,
    dst: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct DownloadFileArgs {
    src: String,
    dst: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct DownloadChunkArgs {
    path: String,
    offset: usize,
    limit: usize,
}

#[derive(Debug, thiserror::Error)]
#[error("file tool error: {0}")]
pub struct FileToolError(String);

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";

    type Error = FileToolError;
    type Args = ReadFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read text content from a file on the selected client.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to read" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        call_remote_tool(
            Arc::clone(&self.manager),
            &self.client_id,
            "read_file",
            serde_json::to_value(args.clone()).map_err(|e| FileToolError(e.to_string()))?,
            Arc::clone(&self.last_output),
        )
        .await
    }
}

impl Tool for WriteFileTool {
    const NAME: &'static str = "write_file";

    type Error = FileToolError;
    type Args = WriteFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write text content to a file on the selected client.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to write" },
                    "content": { "type": "string", "description": "Text content" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        call_remote_tool(
            Arc::clone(&self.manager),
            &self.client_id,
            "write_file",
            serde_json::to_value(args.clone()).map_err(|e| FileToolError(e.to_string()))?,
            Arc::clone(&self.last_output),
        )
        .await
    }
}

impl Tool for UploadFileTool {
    const NAME: &'static str = "upload_file";

    type Error = FileToolError;
    type Args = UploadFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "upload_file".to_string(),
            description: "Upload server-local file (src) to selected client path (dst).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "src": { "type": "string", "description": "Server-local source path" },
                    "dst": { "type": "string", "description": "Remote destination path on client" }
                },
                "required": ["src", "dst"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let bytes = std::fs::read(&args.src)
            .map_err(|e| FileToolError(format!("read server src failed: {e}")))?;
        let chunks = bytes
            .chunks(CHUNK_SIZE)
            .map(|chunk| BASE64.encode(chunk))
            .collect::<Vec<_>>();

        let total = chunks.len();
        for (idx, chunk) in chunks.into_iter().enumerate() {
            let payload = json!({
                "path": args.dst,
                "content_base64": chunk,
                "append": idx > 0,
            });
            call_remote_tool(
                Arc::clone(&self.manager),
                &self.client_id,
                "upload_file",
                payload,
                Arc::clone(&self.last_output),
            )
            .await?;
        }

        let output = json!({
            "tool": "upload_file",
            "src": args.src,
            "dst": args.dst,
            "size_bytes": bytes.len(),
            "chunks": total,
            "note": "content omitted from model context",
        });
        if let Ok(mut guard) = self.last_output.lock() {
            *guard = Some(output.clone());
        }
        Ok(output)
    }
}

impl Tool for DownloadFileTool {
    const NAME: &'static str = "download_file";

    type Error = FileToolError;
    type Args = DownloadFileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "download_file".to_string(),
            description: "Download client file (src) and save to server-local path (dst).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "src": { "type": "string", "description": "Remote source path on client" },
                    "dst": { "type": "string", "description": "Server-local destination path" }
                },
                "required": ["src", "dst"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut offset = 0usize;
        let mut bytes = Vec::new();
        loop {
            let payload = DownloadChunkArgs {
                path: args.src.clone(),
                offset,
                limit: CHUNK_SIZE,
            };
            let response = call_remote_tool(
                Arc::clone(&self.manager),
                &self.client_id,
                "download_file_chunk",
                serde_json::to_value(payload).map_err(|e| FileToolError(e.to_string()))?,
                Arc::clone(&self.last_output),
            )
            .await?;

            let result = response
                .get("result")
                .ok_or_else(|| FileToolError("download_file_chunk missing result".to_string()))?;
            let content_base64 = result
                .get("content_base64")
                .and_then(Value::as_str)
                .ok_or_else(|| FileToolError("download_file_chunk missing content_base64".to_string()))?;
            let chunk = BASE64
                .decode(content_base64.as_bytes())
                .map_err(|e| FileToolError(format!("base64 decode failed: {e}")))?;
            let eof = result
                .get("eof")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let read_len = result
                .get("bytes_read")
                .and_then(Value::as_u64)
                .unwrap_or(chunk.len() as u64) as usize;

            if read_len == 0 {
                break;
            }

            bytes.extend_from_slice(&chunk[..read_len.min(chunk.len())]);
            offset += read_len;

            if eof {
                break;
            }
        }

        if let Some(parent) = Path::new(&args.dst).parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| FileToolError(format!("create dst parent failed: {e}")))?;
        }
        std::fs::write(&args.dst, &bytes)
            .map_err(|e| FileToolError(format!("write dst failed: {e}")))?;

        let output = json!({
            "tool": "download_file",
            "src": args.src,
            "dst": args.dst,
            "size_bytes": bytes.len(),
            "note": "content omitted from model context",
        });
        if let Ok(mut guard) = self.last_output.lock() {
            *guard = Some(output.clone());
        }
        Ok(output)
    }
}

async fn call_remote_tool(
    manager: Arc<Mutex<ClientManager>>,
    client_id: &str,
    tool_name: &str,
    args: Value,
    last_output: Arc<Mutex<Option<Value>>>,
) -> Result<Value, FileToolError> {
    let receiver = {
        let mut mgr = manager
            .lock()
            .map_err(|_| FileToolError("client manager lock failed".to_string()))?;
        let (_, receiver) = mgr
            .dispatch_tool_call(client_id, tool_name, args.clone(), Some(60_000))
            .map_err(FileToolError)?;
        receiver
    };

    let result = tokio::time::timeout(Duration::from_secs(60), receiver)
        .await
        .map_err(|_| FileToolError(format!("{tool_name} timed out")))?
        .map_err(|_| FileToolError(format!("{tool_name} result channel closed")))?;

    if !result.ok {
        return Err(FileToolError(format!("{tool_name} failed: {}", result.error)));
    }

    let capture = json!({
        "tool": tool_name,
        "args": args,
        "result": result.data,
    });
    if let Ok(mut guard) = last_output.lock() {
        *guard = Some(capture.clone());
    }
    Ok(capture)
}
