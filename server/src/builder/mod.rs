use crate::config::ServerConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

pub fn generate_client_binary(
    config: &ServerConfig,
    target: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut command = Command::new("cargo");
    command.args(["build", "-p", "vectorshell-client", "--release"]);

    if let Some(target) = target {
        command.args(["--target", target]);
    }

    let status = command
        .env("VECTOR_SERVER_URL", &config.client.default_server)
        .env("VECTOR_AUTH_TOKEN", &config.auth.token)
        .env("VECTOR_BUILD_UUID", Uuid::new_v4().to_string())
        .env(
            "VECTOR_INSECURE_TLS",
            config.client.insecure_tls.unwrap_or(false).to_string(),
        )
        .env(
            "VECTOR_RECONNECT_INTERVAL",
            config.client.reconnect_interval.to_string(),
        )
        .status()?;

    if !status.success() {
        return Err("client build failed".into());
    }

    let output_dir = Path::new("build").join("clients");
    fs::create_dir_all(&output_dir)?;

    let source_path = client_binary_path(target)?;
    let filename = source_path
        .file_name()
        .ok_or("failed to determine client binary filename")?;
    let dest_path = output_dir.join(filename);

    fs::copy(&source_path, &dest_path)?;
    tracing::info!("client binary written to {}", dest_path.display());
    Ok(())
}

fn client_binary_path(
    target: Option<&str>,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let mut path = PathBuf::from(target_dir);

    if let Some(target) = target {
        path.push(target);
    }
    path.push("release");

    let mut filename = "vectorshell-client".to_string();
    if target
        .map(|t| t.contains("windows"))
        .unwrap_or(cfg!(windows))
    {
        filename.push_str(".exe");
    }
    path.push(filename);

    if !path.exists() {
        return Err(format!("client binary not found at {}", path.display()).into());
    }
    Ok(path)
}
