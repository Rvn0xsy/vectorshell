use crate::client_manager::ExecHistoryEntry;
use crate::client_manager::ClientManager;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::ui::{ui_print, UiState};

#[derive(Clone)]
pub struct ExecTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<ExecOutput>>>,
    ui_state: Arc<Mutex<UiState>>,
}

impl ExecTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<ExecOutput>>>,
        ui_state: Arc<Mutex<UiState>>,
    ) -> Self {
        Self {
            manager,
            client_id,
            last_output,
            ui_state,
        }
    }
}

#[derive(Deserialize)]
pub struct ExecArgs {
    command: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ExecOutput {
    command: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
    cwd: String,
    env: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
#[error("exec tool error: {0}")]
pub struct ExecToolError(String);

impl Tool for ExecTool {
    const NAME: &'static str = "exec";

    type Error = ExecToolError;
    type Args = ExecArgs;
    type Output = ExecOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "exec".to_string(),
            description: "Execute a shell command on the selected client and return stdout/stderr/exit_code.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute on the remote client"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let receiver = {
            let mut mgr = self
                .manager
                .lock()
                .map_err(|_| ExecToolError("client manager lock failed".to_string()))?;
            ui_print(&self.ui_state, "Exec", &args.command);
            mgr.dispatch_exec(&self.client_id, &args.command)
                .map_err(ExecToolError)?
        };

        let entry = tokio::time::timeout(Duration::from_secs(60), receiver)
            .await
            .map_err(|_| ExecToolError("exec timed out".to_string()))?
            .map_err(|_| ExecToolError("exec result channel closed".to_string()))?;

        ui_print(
            &self.ui_state,
            "Result",
            &format!(
                "exit_code={} duration_ms={} cwd={}",
                entry.exit_code, entry.duration_ms, entry.cwd
            ),
        );
        if !entry.stdout.is_empty() {
            ui_print(&self.ui_state, "Result", &format!("stdout: {}", entry.stdout));
        }
        if !entry.stderr.is_empty() {
            ui_print(&self.ui_state, "Result", &format!("stderr: {}", entry.stderr));
        }

        let output = entry_to_output(entry);
        if let Ok(mut guard) = self.last_output.lock() {
            *guard = Some(output.clone());
        }
        Ok(output)
    }
}

fn entry_to_output(entry: ExecHistoryEntry) -> ExecOutput {
    ExecOutput {
        command: entry.command,
        stdout: entry.stdout,
        stderr: entry.stderr,
        exit_code: entry.exit_code,
        duration_ms: entry.duration_ms,
        cwd: entry.cwd,
        env: entry.env,
    }
}
