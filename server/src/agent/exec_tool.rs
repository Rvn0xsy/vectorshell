use crate::client_manager::ClientManager;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::ui::{ui_print, UiState};

#[derive(Clone)]
pub struct ExecTool {
    manager: Arc<Mutex<ClientManager>>,
    client_id: String,
    last_output: Arc<Mutex<Option<Value>>>,
    ui_state: Arc<Mutex<UiState>>,
}

impl ExecTool {
    pub fn new(
        manager: Arc<Mutex<ClientManager>>,
        client_id: String,
        last_output: Arc<Mutex<Option<Value>>>,
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
            let call_args = args.command.clone();
            let (_, receiver) = mgr
                .dispatch_tool_call(
                    &self.client_id,
                    "exec",
                    serde_json::json!({ "command": call_args }),
                    Some(60_000),
                )
                .map_err(ExecToolError)?
                ;
            receiver
        };

        let result = tokio::time::timeout(Duration::from_secs(60), receiver)
            .await
            .map_err(|_| ExecToolError("exec timed out".to_string()))?
            .map_err(|_| ExecToolError("exec result channel closed".to_string()))?;

        if !result.ok {
            return Err(ExecToolError(format!("exec failed: {}", result.error)));
        }

        let output: ExecOutput = serde_json::from_value(result.data)
            .map_err(|e| ExecToolError(format!("invalid exec output: {e}")))?;

        ui_print(
            &self.ui_state,
            "Result",
            &format!(
                "exit_code={} duration_ms={} cwd={}",
                output.exit_code, output.duration_ms, output.cwd
            ),
        );
        if !output.stdout.is_empty() {
            ui_print(&self.ui_state, "Result", &format!("stdout: {}", output.stdout));
        }
        if !output.stderr.is_empty() {
            ui_print(&self.ui_state, "Result", &format!("stderr: {}", output.stderr));
        }

        if let Ok(mut guard) = self.last_output.lock() {
            *guard = Some(json!({
                "tool": "exec",
                "args": {"command": output.command.clone()},
                "result": serde_json::to_value(&output).unwrap_or(Value::Null),
            }));
        }
        Ok(output)
    }
}
