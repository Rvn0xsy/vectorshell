use shared::protocol::{ExecMessage, ResultMessage};
use std::time::Instant;
use tokio::process::Command;

pub async fn execute_command(exec: ExecMessage) -> ResultMessage {
    let start = Instant::now();
    let command_string = exec.command.clone();
    let mut command = if cfg!(windows) {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/c").arg(command_string.clone());
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command_string.clone());
        cmd
    };

    let output = command.output().await;
    let duration_ms = start.elapsed().as_millis() as u64;
    let cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "".to_string());
    let env = std::env::vars().collect::<Vec<_>>();

    let client_id = std::env::var("HOSTNAME").unwrap_or_else(|_| "client".to_string());
    match output {
        Ok(output) => ResultMessage {
            client_id,
            command: command_string,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            duration_ms,
            cwd,
            env,
        },
        Err(error) => ResultMessage {
            client_id,
            command: command_string,
            stdout: String::new(),
            stderr: error.to_string(),
            exit_code: -1,
            duration_ms,
            cwd,
            env,
        },
    }
}
