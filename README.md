# VectorShell

VectorShell is an AI-driven remote command execution platform written in Rust.

```
┌─────────────────┐     WebSocket      ┌─────────────────┐
│  VectorShell    │◄──────────────────►│    Client       │
│    Server       │    (reverse conn)  │   (target host) │
│                 │                    │                 │
│  - AI Agent     │                    │  - Exec tools   │
│  - REST API     │                    │  - File ops     │
│  - SSE events   │                    │  - Report back  │
│  - Web UI (/ui) │                    │                 │
└─────────────────┘                    └─────────────────┘
```

## Quick Start

```bash
# Build everything (Rust release + frontend)
make build

# Run the server
make up

# In another terminal: run a local client
./target/release/vectorshell-client
```

## Makefile Commands

| Command | Description |
|---------|-------------|
| `make build` | Build release binaries (server + client) + frontend |
| `make build-release` | Build Rust release binaries only |
| `make build-server` | Build server only |
| `make build-client` | Build client only |
| `make test` | Run all Rust tests |
| `make up` | Run server with default config |
| `make web-dev` | Start Vite dev server for frontend |
| `make web-build` | Build frontend for production |
| `make gen-client` | Generate client binary (uses config values) |
| `make gen-client TARGET=linux-arm64` | Generate for specific target |
| `make lint` | Run `cargo fmt` + `cargo clippy` |
| `make clean` | Clean build artifacts |

## Configuration

Copy the example config and fill in your values:

```bash
cp config/config.example.toml config/config.toml
```

Key config sections:

```toml
[server]
listen = "0.0.0.0:8080"     # API + WebSocket listen address
ws_path = "/ws"              # WebSocket endpoint
ui_path = "/ui"             # Frontend URL path (default: /ui)
ui_dist = "dashboard/dist"  # Frontend dist directory

[agent]
model = "gpt-5.2-codex"
base_url = "https://api.openai.com/v1"
api_key = "your-key-here"

[auth]
api_token = "..."           # REST API Bearer token
client_token = "..."        # Client WebSocket auth
```

## Generate Client Binary

Generated clients embed config values at compile time. Re-run after changing `config.toml`.

```bash
# Build + generate client for current platform
make build && make gen-client

# Cross-compile for specific target
make gen-client TARGET=linux-arm64
```

Supported targets: `linux-amd64`, `linux-arm64`, `windows-amd64`, `windows-arm64`, `macos-amd64`, `macos-arm64`

Output: `build/clients/vectorshell-client-<target>`

## Server REPL Commands

After starting the server with `make up`, use these commands:

| Command | Description |
|---------|-------------|
| `/sessions` | List connected clients |
| `/use <install_id>` | Select a client (enters agent mode) |
| `/info` | Show selected session details |
| `/exec <cmd>` | Execute raw command on selected client |
| `/read <path>` | Read file from selected client |
| `/write <path> <content>` | Write file to selected client |
| `/upload <src> <dst>` | Upload server file to client |
| `/download <src> <dst>` | Download client file to server |
| `/tool <name> <json>` | Dispatch generic tool call |
| `/agent <prompt>` | Ask AI for a text response |
| `/clear` | Clear agent context history |
| `/back` | Exit agent mode / unselect client |
| `/clean` | Clear session history + context |
| `/help` | Show all commands |

## Frontend

The built-in web UI is served at the `ui_path` URL (default `/ui`) when `ui_dist` is configured.

Access: `http://<server>:<port>/ui`

To develop the frontend locally:

```bash
make web-dev        # Start Vite dev server
# Frontend at http://localhost:5173
# Backend API at http://localhost:8080
```

## TLS / WSS

Enable TLS in `config.toml`:

```toml
[tls]
enabled = true
cert_path = "config/certs/cert.pem"
key_path = "config/certs/key.pem"
```

For self-signed certs, set `client.insecure_tls = true` in config **before** running `generate-client`.

## Windows Proxy

Windows client supports system proxy discovery:

1. WinHTTP auto proxy (PAC/WPAD)
2. Manual proxy from Internet Settings

If resolved, client uses HTTP CONNECT tunnel for WebSocket.

## Architecture

- `server/` — axum API server, LLM agent, WebSocket hub, SQLite persistence
- `client/` — Reverse-connect WebSocket client, executes server-issued tools
- `shared/` — Wire protocol types shared by server and client
- `dashboard/` — React/Vite frontend

## Releases

Binaries are automatically published on git tags matching `v*`:

```bash
git tag v0.0.5 && git push origin v0.0.5
```

Release artifacts include server binary, pre-built clients for all platforms, and the frontend dist.
