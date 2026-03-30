use crate::config::ServerConfig;
use crate::agent::exec_tool::{ExecTool, ToolEventEmitter};
use crate::agent::file_tools::{DownloadFileTool, ReadFileTool, UploadFileTool, WriteFileTool};
use crate::agent::windows_tools::{DotnetAssemblyTool, PowerShellClrTool};
use crate::ui::UiState;
use rig::completion::Prompt;
use rig::providers::openai;
use std::sync::{Arc, Mutex};
use crate::client_manager::ClientManager;
use serde_json::Value;

pub mod exec_tool;
pub mod file_tools;
pub mod windows_tools;

#[derive(Clone)]
pub struct Agent {
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
    identity_prompt: String,
}

impl Agent {
    pub fn new(config: &ServerConfig) -> Self {
        Self {
            model: config.agent.model.clone(),
            base_url: config.agent.base_url.clone(),
            api_key: config.agent.api_key.clone(),
            identity_prompt: load_identity_prompt(),
        }
    }

    pub async fn respond_text(&self, task: &str) -> Result<String, anyhow::Error> {
        let api_key = self
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

        let client = if let Some(base_url) = &self.base_url {
            openai::Client::from_url(&api_key, base_url)
        } else {
            openai::Client::new(&api_key)
        };

        let agent = client.agent(&self.model).build();
        let response = agent
            .prompt(&format!("{}\n\nUser request:\n{}", self.identity_prompt, task))
            .await?;
        let answer = response.trim();
        if answer.is_empty() {
            anyhow::bail!("agent returned empty response");
        }
        Ok(answer.to_string())
    }

    pub async fn respond_with_tools(
        &self,
        task: &str,
        manager: Arc<Mutex<ClientManager>>,
        session_id: &str,
        ui_state: Arc<Mutex<UiState>>,
        event_emitter: Option<ToolEventEmitter>,
    ) -> Result<String, anyhow::Error> {
        let api_key = self
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

        let client = if let Some(base_url) = &self.base_url {
            openai::Client::from_url(&api_key, base_url)
        } else {
            openai::Client::new(&api_key)
        };

        let last_tool_output = Arc::new(Mutex::new(None));
        let exec_tool = ExecTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            ui_state,
            event_emitter.clone(),
        );
        let read_tool = ReadFileTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );
        let write_tool = WriteFileTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );
        let upload_tool = UploadFileTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );
        let download_tool = DownloadFileTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );
        let powershell_clr_tool = PowerShellClrTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );
        let dotnet_assembly_tool = DotnetAssemblyTool::new(
            Arc::clone(&manager),
            session_id.to_string(),
            Arc::clone(&last_tool_output),
            event_emitter.clone(),
        );

        let agent = client
            .agent(&self.model)
            .preamble(&self.identity_prompt)
            .tool(exec_tool)
            .tool(read_tool)
            .tool(write_tool)
            .tool(upload_tool)
            .tool(download_tool)
            .tool(powershell_clr_tool)
            .tool(dotnet_assembly_tool)
            .build();

        let mut tool_outputs: Vec<serde_json::Value> = Vec::new();
        let mut last_call_signature: Option<String> = None;
        let mut repeated_call_count: usize = 0;

        for _ in 0..10 {
            if let Ok(mut guard) = last_tool_output.lock() {
                *guard = None;
            }

            let prompt = if tool_outputs.is_empty() {
                task.to_string()
            } else {
                let outputs = serde_json::to_string(&tool_outputs).unwrap_or_default();
                format!("{}\n\nTool outputs so far:\n{}", task, outputs)
            };

            let response = agent.prompt(&prompt).await?;

            let maybe_output = if let Ok(mut guard) = last_tool_output.lock() {
                guard.take()
            } else {
                None
            };

            if let Some(output) = maybe_output {
                    let signature = tool_call_signature(&output);
                    if let Some(sig) = signature {
                        if last_call_signature.as_deref() == Some(sig.as_str()) {
                            repeated_call_count += 1;
                        } else {
                            repeated_call_count = 1;
                            last_call_signature = Some(sig);
                        }

                        if repeated_call_count >= 3 {
                            let forced_prompt = format!(
                                "{}\n\nYou have already executed the same tool call repeatedly and received results. Stop calling tools now. Provide final concise answer to user.",
                                prompt
                            );
                            let forced_response = agent.prompt(&forced_prompt).await?;
                            let forced_answer = forced_response.trim();
                            if !forced_answer.is_empty() {
                                return Ok(forced_answer.to_string());
                            }
                            return Err(anyhow::anyhow!(
                                "agent loop detected repeated tool calls and returned empty final answer"
                            ));
                        }
                    }

                    tool_outputs.push(
                        serde_json::to_value(output).unwrap_or_else(|_| serde_json::Value::Null),
                    );
                    continue;
            }

            let answer = response.trim();
            if !answer.is_empty() {
                return Ok(answer.to_string());
            }
        }

        Err(anyhow::anyhow!("agent did not produce a final answer"))
    }
}

fn tool_call_signature(output: &Value) -> Option<String> {
    let tool = output.get("tool")?.as_str()?;
    let args = output.get("args").cloned().unwrap_or(Value::Null);
    Some(format!("{}:{}", tool, args))
}

fn load_identity_prompt() -> String {
    let soul_path = std::path::Path::new("config/SOUL.md");
    if soul_path.exists() {
        if let Ok(content) = std::fs::read_to_string(soul_path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    default_identity_prompt()
}

fn default_identity_prompt() -> String {
    [
        "You are VectorShell Agent, a reliable remote systems operator.",
        "Goal: complete user tasks safely and efficiently on selected client hosts.",
        "",
        "Available tools:",
        "- exec(command): run shell command on selected client.",
        "- read_file(path): read text file on selected client.",
        "- write_file(path,content): write text file on selected client.",
        "- upload_file(src,dst): upload server-local file src to client path dst.",
        "- download_file(src,dst): download client path src to server-local path dst.",
        "- powershell_clr(script): run PowerShell via CLR host (Windows clients only).",
        "- dotnet_assembly(content_base64|artifact_id,runtime_version,args,domain,patch_exit): run .NET EXE in-memory (Windows clients only).",
        "",
        "Tool usage policy:",
        "- Prefer file tools for file operations; use exec only when file tools are insufficient.",
        "- Use windows-only tools ONLY when selected client capability includes them.",
        "- upload_file/download_file are directional; do not confuse src/dst sides.",
        "- read_file/write_file paths are on CLIENT side.",
        "",
        "Language mapping hints:",
        "- Chinese: '上传 A 到 B' => upload_file(src=A,dst=B).",
        "- Chinese: '下载 A 到 B' => download_file(src=A,dst=B).",
        "",
        "Behavior constraints:",
        "- Avoid repeated identical tool calls when result already confirms completion.",
        "- If required parameters are missing, ask one concise clarification question.",
        "- Keep final answers concise and action-focused.",
    ]
    .join("\n")
}
