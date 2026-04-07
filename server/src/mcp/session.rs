//! MCP session state management.

use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// MCP session state for one connected MCP client.
#[derive(Debug, Clone)]
pub struct McpSession {
    /// Unique MCP session identifier.
    pub session_id: String,
    /// Whether the client has completed the initialize handshake.
    pub initialized: bool,
    /// The MCP protocol version the client requested.
    pub protocol_version: Option<String>,
}

impl McpSession {
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            initialized: false,
            protocol_version: None,
        }
    }
}

impl Default for McpSession {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory store of active MCP sessions.
#[derive(Default)]
pub struct McpSessionStore {
    sessions: Mutex<HashMap<String, McpSession>>,
}

impl McpSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new session and return its ID.
    pub fn create(&self) -> String {
        let session = McpSession::new();
        let id = session.session_id.clone();
        self.sessions.lock().unwrap().insert(id.clone(), session);
        id
    }

    /// Get a session by ID.
    pub fn get(&self, id: &str) -> Option<McpSession> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    /// Check if a session exists and is initialized.
    pub fn is_initialized(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.initialized)
            .unwrap_or(false)
    }

    /// Mark a session as initialized.
    pub fn mark_initialized(&self, id: &str, protocol_version: Option<String>) {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
            session.initialized = true;
            session.protocol_version = protocol_version;
        }
    }

    /// Remove a session.
    pub fn remove(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }
}
