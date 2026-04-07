# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development commands

### Makefile (primary interface)
- `make build` — Build release binaries (server + client) + frontend
- `make up` — Run server with default config (`config/config.toml`)
- `make test` — Run all Rust tests
- `make web-dev` — Start Vite dev server for frontend
- `make web-build` — Build frontend for production
- `make gen-client` — Generate client binary
- `make lint` — Run `cargo fmt` && `cargo clippy`

### Rust workspace
- `cargo build` — Build everything (dev profile)
- `cargo build --release` — Build release binaries
- `cargo build -p vectorshell-server` — Build server only
- `cargo build -p vectorshell-client` — Build client only
- `cargo test` — Run all Rust tests
- `cargo test -p shared` — Run tests for one crate
- `cargo test -p shared <test_name> -- --exact` — Run a single test
- `cargo run -p vectorshell-server -- --config config/config.toml` — Run server
- `cargo run -p vectorshell-server -- --help` — Show CLI help
- `cargo run -p vectorshell-server -- --config config/config.toml generate-client [--target <triple>]` — Generate client binary

### Web app
- `npm install --prefix dashboard` — Install deps
- `npm run --prefix dashboard dev` — Start Vite dev server
- `npm run --prefix dashboard build` — Build for production
- `npm run --prefix dashboard lint` — Lint

## Configuration and runtime shape
- Main server config: `config/config.toml`; examples: `config/config.example.toml`, `config/config.test.toml`.
- `config/SOUL.md` is the agent preamble loaded when present (falls back to built-in default).
- `server/src/config` deserializes: `[server]`, `[agent]`, `[client]`, `[auth]`, `[tls]`, `[mcp]`.
- The server can serve both the API and the built frontend on the same port; the frontend is mounted at `ui_path` (default `/ui`) from `ui_dist` (default `dashboard/dist`).
- Client binaries are parameterized at build time via compile-time `env!` constants (`VECTOR_SERVER_URL`, `VECTOR_AUTH_TOKEN`, `VECTOR_BUILD_UUID`, `VECTOR_INSECURE_TLS`, `VECTOR_RECONNECT_INTERVAL`). The server's `generate-client` path sets these before `cargo build -p vectorshell-client --release`.
- Persistent SQLite database: `data/vectorshell.db` (created automatically).
- Logs: `logs/vectorshell.log` (rolling daily, via `tracing-appender`).

## High-level architecture

### Workspace structure
- `server/`: axum-based control plane, API, LLM agent, and MCP server.
- `client/`: remote client — reverse WebSocket connect, executes server-issued tools.
- `shared/`: wire protocol types (`shared/src/protocol.rs`) shared by server and client.
- `dashboard/`: React/Vite frontend.

### Server architecture
- `server/src/main.rs`: entrypoint. Loads config, constructs `Agent`, `ClientManager`, SQLite `Db`, event bus, and UI state, then runs the API server and local REPL together.
- `server/src/api/mod.rs`: HTTP/WebSocket boundary. Handles client WebSocket registration, authenticated REST endpoints (sessions, conversations, artifacts), SSE streams, MCP server (`/mcp`), and static asset serving.
- `server/src/client_manager/mod.rs`: in-memory live-session registry. Stores active connections, capabilities, pending requests, and exec history.
- `server/src/db/mod.rs`: SQLite persistent layer (`data/vectorshell.db`). Session presence, command/chat history, conversation-to-install mappings.
- `server/src/event_bus/mod.rs`: per-key broadcast hub. SSE session streams and conversation streams are keyed separately in-memory.
- `server/src/builder/mod.rs`: wraps client cross-compilation, embeds config, outputs to `build/clients/`.
- `server/src/ui.rs`: REPL/internal UI state helpers.
- `server/src/mcp/`: MCP 2025-11-25 server implementation (`mod.rs`, `protocol.rs`, `session.rs`, `tools.rs`).
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
- `api_token` (REST): Bearer token for HTTP API endpoints and MCP server auth.
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

### MCP Server
The MCP server exposes VectorShell tools to MCP-compatible AI clients (e.g., Claude Desktop):
- `POST /mcp` — JSON-RPC 2.0 requests (`initialize`, `tools/list`, `tools/call`)
- `GET /mcp` — SSE keepalive stream
- Auth: uses `api_token` from config (same as REST API)
- Route wired in `server/src/api/mod.rs` via `mcp_handle_jsonrpc` and `mcp_sse`
- Tool definitions in `server/src/mcp/tools.rs`; JSON-RPC protocol in `server/src/mcp/protocol.rs`
