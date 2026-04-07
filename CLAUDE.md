# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development commands

### Rust workspace
- Build everything: `cargo build`
- Build release binaries: `cargo build --release`
- Build just the server: `cargo build -p vectorshell-server`
- Build just the client: `cargo build -p vectorshell-client`
- Run all Rust tests: `cargo test`
- Run tests for one crate: `cargo test -p shared`
- Run a single Rust test: `cargo test -p shared register_message_roundtrip -- --exact`
- Run the server: `cargo run -p vectorshell-server -- --config config/config.toml`
- Show server CLI help: `cargo run -p vectorshell-server -- --help`
- Generate a client binary: `cargo run -p vectorshell-server -- --config config/config.toml generate-client`
- Cross-compile a client for a specific target:
  `cargo run -p vectorshell-server -- --config config/config.toml generate-client --target linux-amd64`
  Supported aliases: `linux-amd64`, `linux-arm64`, `windows-amd64`, `windows-arm64`, `macos-amd64`, `macos-arm64` (or use raw triples like `x86_64-unknown-linux-gnu`).

### Web app
- Install deps: `npm install --prefix dashboard`
- Start the Vite dev server: `npm run --prefix dashboard dev`
- Build the web app: `npm run --prefix dashboard build`
- Lint the web app: `npm run --prefix dashboard lint`
- Preview the production web build: `npm run --prefix dashboard preview`

## Configuration and runtime shape
- Main server config: `config/config.toml`; examples: `config/config.example.toml`, `config/config.test.toml`.
- `config/SOUL.md` is the agent preamble loaded when present (falls back to built-in default).
- `server/src/config` deserializes: `[server]` (listen, ws_path, ui_path, ui_dist), `[agent]`, `[client]`, `[auth]`, `[tls]`.
- The server can serve both the API and the built frontend on the same port; the frontend is mounted at `ui_path` (default `/ui`) from `ui_dist` (default `dashboard/dist`).
- Client binaries are parameterized at build time via compile-time `env!` constants (`VECTOR_SERVER_URL`, `VECTOR_AUTH_TOKEN`, `VECTOR_BUILD_UUID`, `VECTOR_INSECURE_TLS`, `VECTOR_RECONNECT_INTERVAL`). The server’s `generate-client` path sets these before `cargo build -p vectorshell-client --release`.
- Persistent SQLite database: `data/vectorshell.db` (created automatically).
- Logs: `logs/vectorshell.log` (rolling daily, via `tracing-appender`).

## High-level architecture

### Workspace structure
- `server/`: axum-based control plane, API, and LLM agent.
- `client/`: remote client — reverse WebSocket connect, executes server-issued tools.
- `shared/`: wire protocol types (`shared/src/protocol.rs`) shared by server and client.
- `dashboard/`: React/Vite frontend.

### Server architecture
- `server/src/main.rs`: entrypoint. Loads config, constructs `Agent`, `ClientManager`, SQLite `Db`, event bus, and UI state, then runs the API server and local REPL together.
- `server/src/api/mod.rs`: HTTP/WebSocket boundary. Handles client WebSocket registration, authenticated REST endpoints (sessions, conversations, artifacts), SSE streams, and static asset serving.
- `server/src/client_manager/mod.rs`: in-memory live-session registry. Stores active connections, capabilities, pending requests, and exec history.
- `server/src/db/mod.rs`: SQLite persistent layer (`data/vectorshell.db`). Session presence, command/chat history, conversation-to-install mappings.
- `server/src/event_bus/mod.rs`: per-key broadcast hub. SSE session streams and conversation streams are keyed separately in-memory.
- `server/src/builder/mod.rs`: wraps client cross-compilation, embeds config, outputs to `build/clients/`.
- `server/src/ui.rs`: REPL/internal UI state helpers.
- Tool implementations: `server/src/agent/exec_tool.rs`, `server/src/agent/file_tools.rs`, `server/src/agent/windows_tools.rs`.

### Agent/tool flow
- The server agent runs **only on the server**; tools are marshaled through `ClientManager` to the selected connected client.
- The agent loop maintains a small in-memory history list and a repeated-call guard.
- Windows-only tools (PowerShell CLR, .NET assembly) are gated by client capability advertisement.

### Client architecture
- `client/src/main.rs`: thin wrapper around `websocket::run_client()`.
- `client/src/websocket/mod.rs`: reconnect behavior, WebSocket registration, heartbeats, server message dispatch.
- `client/src/executor.rs`: execution backend — shell exec, file read/write/upload/download, chunked download, Windows CLR/.NET helpers.
- `client/src/embedded_config/mod.rs`: compile-time `env!` constants (server URL, auth token, reconnect interval, insecure TLS).

### Session identity layers (critical)
- `connection_id`: identifies a live WebSocket connection.
- `session_id`: returned by server on registration, used for live routing.
- `install_id`: persisted across reconnects; all persistent history (chat, command) is keyed by `install_id`.
- The REPL `/use <install_id>` maps to `session_id` via `ClientManager::get_by_install_id`.

### Auth tokens (two separate)
- `api_token` (REST): Bearer token for HTTP API endpoints.
- `client_token` (WebSocket): sent by client at registration time on the WebSocket connection.

### Shared protocol (`shared/src/protocol.rs`)
- Tagged enum messages over WebSocket JSON.
- Client→Server: `Register`, `Heartbeat`, `Result`, `ToolResult`.
- Server→Client: `Exec`, `Upload`, `Download`, `Ping`, `ToolCall`, `Registered`.
- If you change wire shapes here, coordinated server+client updates are required.

### Frontend architecture
- `dashboard/src/App.tsx`: main UI component, holds most state.
- Two SSE channels: session stream (exec/tool results for selected connection) and conversation stream (agent/tool lifecycle events).
- Session selection, conversation IDs, API base URL, and auth token are cached in browser local storage.
- The frontend talks only to the server HTTP API; it does not connect directly to clients.

### API docs
- `api-docs/README.md` and `api-docs/openapi.yaml` describe the REST/SSE surface. Refer to these when changing frontend/server API behavior.
