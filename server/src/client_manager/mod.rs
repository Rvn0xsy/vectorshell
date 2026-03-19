use shared::protocol::{ExecMessage, ServerToClientMessage, ToolCallMessage, ToolResultMessage};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ClientMetadata {
    pub client_id: String,
    pub connection_id: String,
    pub install_id: String,
    pub build_uuid: String,
    pub last_heartbeat: u64,
    pub hostname: String,
    pub username: String,
    pub pid: u32,
    pub os: String,
    pub arch: String,
    pub ip: String,
    pub registered_at: u64,
    pub capabilities: Vec<String>,
}

impl ClientMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client_id: String,
        connection_id: String,
        install_id: String,
        build_uuid: String,
        hostname: String,
        username: String,
        pid: u32,
        os: String,
        arch: String,
        ip: String,
        registered_at: u64,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            client_id,
            connection_id,
            install_id,
            build_uuid,
            last_heartbeat: 0,
            hostname,
            username,
            pid,
            os,
            arch,
            ip,
            registered_at,
            capabilities,
        }
    }
}

#[derive(Debug)]
pub struct ClientConnection {
    pub sender: mpsc::UnboundedSender<ServerToClientMessage>,
    pub metadata: ClientMetadata,
}

#[derive(Debug, Clone)]
pub struct ExecHistoryEntry {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct ClientManager {
    clients: HashMap<String, ClientConnection>,
    exec_history: HashMap<String, Vec<ExecHistoryEntry>>,
    pending_exec: HashMap<String, oneshot::Sender<ExecHistoryEntry>>,
    pending_tool: HashMap<String, oneshot::Sender<ToolResultMessage>>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            exec_history: HashMap::new(),
            pending_exec: HashMap::new(),
            pending_tool: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        client_id: String,
        sender: mpsc::UnboundedSender<ServerToClientMessage>,
        metadata: ClientMetadata,
    ) {
        tracing::info!(client_id = %metadata.client_id, connection_id = %metadata.connection_id, "client connected");
        self.clients
            .insert(client_id, ClientConnection { sender, metadata });
    }

    pub fn list_clients(&self) -> Vec<ClientMetadata> {
        let mut list = self
            .clients
            .values()
            .map(|c| c.metadata.clone())
            .collect::<Vec<_>>();
        list.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        list
    }

    pub fn get_client_metadata(&self, client_id: &str) -> Option<ClientMetadata> {
        self.clients
            .get(client_id)
            .map(|conn| conn.metadata.clone())
    }

    pub fn get_by_connection_id(&self, connection_id: &str) -> Option<ClientMetadata> {
        self.clients
            .values()
            .find(|conn| conn.metadata.connection_id == connection_id)
            .map(|conn| conn.metadata.clone())
    }

    pub fn record_exec_result(&mut self, client_id: &str, exec_id: &str, entry: ExecHistoryEntry) {
        self.exec_history
            .entry(client_id.to_string())
            .or_default()
            .push(entry);

        if exec_id.is_empty() {
            return;
        }

        if let Some(sender) = self.pending_exec.remove(exec_id) {
            let _ = sender.send(
                self.exec_history
                    .get(client_id)
                    .and_then(|v| v.last().cloned())
                    .unwrap_or(ExecHistoryEntry {
                        command: String::new(),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: -1,
                        duration_ms: 0,
                        cwd: String::new(),
                        env: Vec::new(),
                    }),
            );
        }
    }

    pub fn record_tool_result(&mut self, tool_id: &str, result: ToolResultMessage) {
        if let Some(sender) = self.pending_tool.remove(tool_id) {
            let _ = sender.send(result);
        }
    }

    pub fn get_exec_history(&self, client_id: &str) -> Vec<ExecHistoryEntry> {
        self.exec_history
            .get(client_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn send_exec(&self, client_id: &str, command: &str) -> Result<(), String> {
        let connection = self
            .clients
            .get(client_id)
            .ok_or_else(|| "client not found".to_string())?;

        tracing::info!(client_id = %client_id, command = %command, "dispatching exec");
        let msg = ServerToClientMessage::Exec {
            id: Uuid::new_v4().to_string(),
            payload: ExecMessage {
                command: command.to_string(),
            },
        };

        connection
            .sender
            .send(msg)
            .map_err(|_| "failed to send command".to_string())
    }

    pub fn dispatch_tool_call(
        &mut self,
        client_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        timeout_ms: Option<u64>,
    ) -> Result<(String, oneshot::Receiver<ToolResultMessage>), String> {
        let connection = self
            .clients
            .get(client_id)
            .ok_or_else(|| "client not found".to_string())?;

        if !connection.metadata.capabilities.is_empty()
            && !connection
                .metadata
                .capabilities
                .iter()
                .any(|cap| cap == tool_name)
        {
            return Err(format!(
                "client does not support tool '{}'; capabilities={}",
                tool_name,
                connection.metadata.capabilities.join(",")
            ));
        }

        let tool_id = Uuid::new_v4().to_string();
        let msg = ServerToClientMessage::ToolCall {
            id: tool_id.clone(),
            payload: ToolCallMessage {
                tool_name: tool_name.to_string(),
                args,
                timeout_ms,
            },
        };

        connection
            .sender
            .send(msg)
            .map_err(|_| "failed to send tool call".to_string())?;

        let (tx, rx) = oneshot::channel();
        self.pending_tool.insert(tool_id.clone(), tx);
        Ok((tool_id, rx))
    }

    pub fn update_heartbeat(&mut self, client_id: &str, timestamp: u64) {
        if let Some(conn) = self.clients.get_mut(client_id) {
            conn.metadata.last_heartbeat = timestamp;
        }
    }
}
