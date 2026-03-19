use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::protocol::{ExecMessage, ResultMessage, ToolCallMessage, ToolResultMessage};
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct ReadArgs {
    path: String,
}

#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct UploadArgs {
    path: String,
    content_base64: String,
    #[serde(default)]
    append: bool,
}

#[derive(Debug, Deserialize)]
struct DownloadArgs {
    path: String,
}

#[derive(Debug, Deserialize)]
struct DownloadChunkArgs {
    path: String,
    offset: usize,
    limit: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecData {
    command: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
    cwd: String,
    env: Vec<(String, String)>,
}

pub fn capabilities() -> Vec<String> {
    vec![
        "exec".to_string(),
        "read_file".to_string(),
        "write_file".to_string(),
        "upload_file".to_string(),
        "download_file".to_string(),
        "download_file_chunk".to_string(),
    ]
}

pub async fn execute_command(client_id: &str, exec: ExecMessage) -> ResultMessage {
    let start = Instant::now();
    let command_string = exec.command.clone();
    let mut command = if cfg!(windows) {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/c").arg(command_string.clone());
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command_string.clone());
        cmd
    };

    let output = command.output().await;
    let duration_ms = start.elapsed().as_millis() as u64;
    let cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "".to_string());
    let env = std::env::vars().collect::<Vec<_>>();

    match output {
        Ok(output) => ResultMessage {
            client_id: client_id.to_string(),
            command: command_string,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            duration_ms,
            cwd,
            env,
        },
        Err(error) => ResultMessage {
            client_id: client_id.to_string(),
            command: command_string,
            stdout: String::new(),
            stderr: error.to_string(),
            exit_code: -1,
            duration_ms,
            cwd,
            env,
        },
    }
}

pub async fn execute_tool(client_id: &str, call: &ToolCallMessage) -> ToolResultMessage {
    let start = Instant::now();
    let result = match call.tool_name.as_str() {
        "exec" => tool_exec(&call.args).await,
        "read_file" => tool_read_file(&call.args).await,
        "write_file" => tool_write_file(&call.args).await,
        "upload_file" => tool_upload_file(&call.args).await,
        "download_file" => tool_download_file(&call.args).await,
        "download_file_chunk" => tool_download_file_chunk(&call.args).await,
        other => Err(format!("unsupported tool: {other}")),
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(data) => ToolResultMessage {
            client_id: client_id.to_string(),
            tool_name: call.tool_name.clone(),
            ok: true,
            data,
            error: String::new(),
            duration_ms,
        },
        Err(error) => ToolResultMessage {
            client_id: client_id.to_string(),
            tool_name: call.tool_name.clone(),
            ok: false,
            data: Value::Null,
            error,
            duration_ms,
        },
    }
}

async fn tool_exec(args: &Value) -> Result<Value, String> {
    let exec_args: ExecMessage = serde_json::from_value(args.clone())
        .map_err(|e| format!("invalid exec args: {e}"))?;
    let result = execute_command("tool-exec", exec_args).await;
    let data = ExecData {
        command: result.command,
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        cwd: result.cwd,
        env: result.env,
    };
    serde_json::to_value(data).map_err(|e| e.to_string())
}

async fn tool_read_file(args: &Value) -> Result<Value, String> {
    let read_args: ReadArgs =
        serde_json::from_value(args.clone()).map_err(|e| format!("invalid read args: {e}"))?;
    let content = fs::read_to_string(&read_args.path)
        .await
        .map_err(|e| format!("read_file failed: {e}"))?;
    Ok(json!({ "path": read_args.path, "content": content }))
}

async fn tool_write_file(args: &Value) -> Result<Value, String> {
    let write_args: WriteArgs =
        serde_json::from_value(args.clone()).map_err(|e| format!("invalid write args: {e}"))?;
    if let Some(parent) = PathBuf::from(&write_args.path).parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("create parent failed: {e}"))?;
    }
    fs::write(&write_args.path, write_args.content.as_bytes())
        .await
        .map_err(|e| format!("write_file failed: {e}"))?;
    Ok(json!({ "path": write_args.path, "bytes_written": write_args.content.len() }))
}

async fn tool_upload_file(args: &Value) -> Result<Value, String> {
    let upload_args: UploadArgs = serde_json::from_value(args.clone())
        .map_err(|e| format!("invalid upload args: {e}"))?;
    let bytes = BASE64
        .decode(upload_args.content_base64.as_bytes())
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    if let Some(parent) = PathBuf::from(&upload_args.path).parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("create parent failed: {e}"))?;
    }
    if upload_args.append {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&upload_args.path)
            .await
            .map_err(|e| format!("upload open failed: {e}"))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("upload append failed: {e}"))?;
    } else {
        fs::write(&upload_args.path, &bytes)
            .await
            .map_err(|e| format!("upload write failed: {e}"))?;
    }
    Ok(json!({ "path": upload_args.path, "bytes_written": bytes.len() }))
}

async fn tool_download_file(args: &Value) -> Result<Value, String> {
    let download_args: DownloadArgs = serde_json::from_value(args.clone())
        .map_err(|e| format!("invalid download args: {e}"))?;
    let mut file = fs::File::open(&download_args.path)
        .await
        .map_err(|e| format!("download open failed: {e}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .await
        .map_err(|e| format!("download read failed: {e}"))?;
    Ok(json!({
        "path": download_args.path,
        "content_base64": BASE64.encode(bytes),
    }))
}

async fn tool_download_file_chunk(args: &Value) -> Result<Value, String> {
    let download_args: DownloadChunkArgs = serde_json::from_value(args.clone())
        .map_err(|e| format!("invalid download_chunk args: {e}"))?;

    let file_bytes = fs::read(&download_args.path)
        .await
        .map_err(|e| format!("download_chunk read failed: {e}"))?;

    if download_args.offset >= file_bytes.len() {
        return Ok(json!({
            "path": download_args.path,
            "offset": download_args.offset,
            "bytes_read": 0,
            "eof": true,
            "content_base64": "",
        }));
    }

    let end = (download_args.offset + download_args.limit).min(file_bytes.len());
    let chunk = &file_bytes[download_args.offset..end];
    Ok(json!({
        "path": download_args.path,
        "offset": download_args.offset,
        "bytes_read": chunk.len(),
        "eof": end >= file_bytes.len(),
        "content_base64": BASE64.encode(chunk),
    }))
}

pub async fn decode_tool_result_as_exec(result: &ToolResultMessage) -> Result<ResultMessage, String> {
    if !result.ok {
        return Err(result.error.clone());
    }
    let data: ExecData =
        serde_json::from_value(result.data.clone()).map_err(|e| format!("invalid exec data: {e}"))?;
    Ok(ResultMessage {
        client_id: result.client_id.clone(),
        command: data.command,
        stdout: data.stdout,
        stderr: data.stderr,
        exit_code: data.exit_code,
        duration_ms: data.duration_ms,
        cwd: data.cwd,
        env: data.env,
    })
}
