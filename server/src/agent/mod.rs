use crate::config::ServerConfig;
use crate::agent::exec_tool::ExecTool;
use crate::ui::UiState;
use rig::completion::Prompt;
use rig::providers::openai;
use std::sync::{Arc, Mutex};
use crate::client_manager::ClientManager;

pub mod exec_tool;

pub struct Agent {
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
}

impl Agent {
    pub fn new(config: &ServerConfig) -> Self {
        Self {
            model: config.agent.model.clone(),
            base_url: config.agent.base_url.clone(),
            api_key: config.agent.api_key.clone(),
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
        let response = agent.prompt(task).await?;
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
        client_id: &str,
        ui_state: Arc<Mutex<UiState>>,
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

        let last_exec_output = Arc::new(Mutex::new(None));
        let exec_tool = ExecTool::new(
            Arc::clone(&manager),
            client_id.to_string(),
            Arc::clone(&last_exec_output),
            ui_state,
        );

        let agent = client
            .agent(&self.model)
            .preamble(
                "You are a systems operator. Use the exec tool when needed. When you have enough info, respond with a concise summary.",
            )
            .tool(exec_tool)
            .build();

        let mut tool_outputs: Vec<serde_json::Value> = Vec::new();

        for _ in 0..10 {
            if let Ok(mut guard) = last_exec_output.lock() {
                *guard = None;
            }

            let prompt = if tool_outputs.is_empty() {
                task.to_string()
            } else {
                let outputs = serde_json::to_string(&tool_outputs).unwrap_or_default();
                format!("{}\n\nTool outputs so far:\n{}", task, outputs)
            };

            let response = agent.prompt(&prompt).await?;

            if let Ok(mut guard) = last_exec_output.lock() {
                if let Some(output) = guard.take() {
                    tool_outputs.push(
                        serde_json::to_value(output).unwrap_or_else(|_| serde_json::Value::Null),
                    );
                    continue;
                }
            }

            let answer = response.trim();
            if !answer.is_empty() {
                return Ok(answer.to_string());
            }
        }

        Err(anyhow::anyhow!("agent did not produce a final answer"))
    }
}
