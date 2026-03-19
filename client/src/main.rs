mod embedded_config;
mod executor;
mod websocket;

use websocket::run_client;

#[tokio::main]
async fn main() {
    if let Err(error) = run_client().await {
        eprintln!("client error: {error}");
    }
}
