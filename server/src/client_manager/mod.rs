use shared::protocol::{ExecMessage, ServerToClientMessage};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct ClientMetadata {
    pub client_id: String,
    pub last_heartbeat: u64,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub ip: String,
    pub registered_at: u64,
}

impl ClientMetadata {
    pub fn new(
        client_id: String,
        hostname: String,
        os: String,
        arch: String,
        ip: String,
        registered_at: u64,
    ) -> Self {
        Self {
            client_id,
            last_heartbeat: 0,
            hostname,
            os,
            arch,
            ip,
            registered_at,
        }
    }
}

#[derive(Debug)]
pub struct ClientConnection {
    pub sender: mpsc::UnboundedSender<ServerToClientMessage>,
    pub metadata: ClientMetadata,
}

#[derive(Debug, Default)]
pub struct ClientManager {
    clients: HashMap<String, ClientConnection>,
    exec_history: HashMap<String, Vec<ExecHistoryEntry>>,
    pending_exec: HashMap<String, oneshot::Sender<ExecHistoryEntry>>,
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

impl ClientManager {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            exec_history: HashMap::new(),
            pending_exec: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        client_id: String,
        sender: mpsc::UnboundedSender<ServerToClientMessage>,
        metadata: ClientMetadata,
    ) {
        tracing::info!(client_id = %metadata.client_id, "client connected");
        self.clients
            .insert(client_id, ClientConnection { sender, metadata });
    }

    pub fn list_clients(&self) -> Vec<String> {
        self.clients.keys().cloned().collect()
    }

    pub fn get_client_metadata(&self, client_id: &str) -> Option<ClientMetadata> {
        self.clients
            .get(client_id)
            .map(|conn| conn.metadata.clone())
    }

    pub fn record_exec_result(&mut self, client_id: &str, exec_id: &str, entry: ExecHistoryEntry) {
        self.exec_history
            .entry(client_id.to_string())
            .or_default()
            .push(entry.clone());

        if exec_id.is_empty() {
            return;
        }

        if let Some(sender) = self.pending_exec.remove(exec_id) {
            let _ = sender.send(entry);
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
            id: uuid_v4(),
            payload: ExecMessage {
                command: command.to_string(),
            },
        };

        connection
            .sender
            .send(msg)
            .map_err(|_| "failed to send command".to_string())
    }

    pub fn dispatch_exec(
        &mut self,
        client_id: &str,
        command: &str,
    ) -> Result<oneshot::Receiver<ExecHistoryEntry>, String> {
        let connection = self
            .clients
            .get(client_id)
            .ok_or_else(|| "client not found".to_string())?;

        tracing::info!(client_id = %client_id, command = %command, "dispatching exec (tool)");
        let exec_id = uuid_v4();
        let msg = ServerToClientMessage::Exec {
            id: exec_id.clone(),
            payload: ExecMessage {
                command: command.to_string(),
            },
        };

        connection
            .sender
            .send(msg)
            .map_err(|_| "failed to send command".to_string())?;

        let (tx, rx) = oneshot::channel();
        self.pending_exec.insert(exec_id, tx);
        Ok(rx)
    }

    pub fn update_heartbeat(&mut self, client_id: &str, timestamp: u64) {
        if let Some(conn) = self.clients.get_mut(client_id) {
            conn.metadata.last_heartbeat = timestamp;
        }
    }
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", now.as_secs(), now.subsec_nanos())
}
