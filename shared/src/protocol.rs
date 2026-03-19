use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterMessage {
    pub client_id: String,
    pub token: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub ip: String,
    pub timestamp: u64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub build_uuid: String,
    #[serde(default)]
    pub install_id: String,
    #[serde(default)]
    pub connection_id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecMessage {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultMessage {
    pub client_id: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatMessage {
    pub client_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadMessage {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadMessage {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PingMessage {
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallMessage {
    pub tool_name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultMessage {
    pub client_id: String,
    pub tool_name: String,
    pub ok: bool,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub error: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ClientToServerMessage {
    #[serde(rename = "register")]
    Register {
        id: String,
        payload: RegisterMessage,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat {
        id: String,
        payload: HeartbeatMessage,
    },
    #[serde(rename = "result")]
    Result { id: String, payload: ResultMessage },
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        payload: ToolResultMessage,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ServerToClientMessage {
    #[serde(rename = "exec")]
    Exec { id: String, payload: ExecMessage },
    #[serde(rename = "upload")]
    Upload { id: String, payload: UploadMessage },
    #[serde(rename = "download")]
    Download {
        id: String,
        payload: DownloadMessage,
    },
    #[serde(rename = "ping")]
    Ping { id: String, payload: PingMessage },
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        payload: ToolCallMessage,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn register_message_roundtrip() {
        let msg = ClientToServerMessage::Register {
            id: "abc".to_string(),
            payload: RegisterMessage {
                client_id: "client-1".to_string(),
                token: "secret".to_string(),
                hostname: "host".to_string(),
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                ip: "127.0.0.1".to_string(),
                timestamp: 123,
                username: "user".to_string(),
                pid: 42,
                build_uuid: "build-1".to_string(),
                install_id: "install-1".to_string(),
                connection_id: "conn-1".to_string(),
                capabilities: vec!["exec".to_string()],
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ClientToServerMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(msg, back);
    }

    #[test]
    fn tool_call_roundtrip() {
        let msg = ServerToClientMessage::ToolCall {
            id: "job-1".to_string(),
            payload: ToolCallMessage {
                tool_name: "read_file".to_string(),
                args: json!({"path": "a.txt"}),
                timeout_ms: Some(5000),
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerToClientMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(msg, back);
    }
}
