use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterMessage {
    pub client_id: String,
    pub token: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub ip: String,
    pub timestamp: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ClientToServerMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(msg, back);
    }

    #[test]
    fn exec_message_roundtrip() {
        let msg = ServerToClientMessage::Exec {
            id: "job-1".to_string(),
            payload: ExecMessage {
                command: "whoami".to_string(),
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ServerToClientMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(msg, back);
    }

    #[test]
    fn result_message_roundtrip() {
        let msg = ClientToServerMessage::Result {
            id: "job-2".to_string(),
            payload: ResultMessage {
                client_id: "client-1".to_string(),
                command: "whoami".to_string(),
                stdout: "user".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
                duration_ms: 12,
                cwd: "/home/user".to_string(),
                env: vec![("PATH".to_string(), "/bin".to_string())],
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let back: ClientToServerMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(msg, back);
    }
}
