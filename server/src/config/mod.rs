use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSection,
    pub agent: AgentSection,
    pub client: ClientSection,
    pub auth: AuthSection,
    pub tls: Option<TlsSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSection {
    pub listen: String,
    pub ws_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TlsSection {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientSection {
    pub default_server: String,
    pub reconnect_interval: u64,
    pub insecure_tls: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthSection {
    pub api_token: String,
    pub client_token: String,
}

impl ServerConfig {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let contents = fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }
}
