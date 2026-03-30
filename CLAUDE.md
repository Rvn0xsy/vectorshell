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
- Run the server with the default config: `cargo run -p vectorshell-server -- --config config/config.toml`
- Show server CLI help: `cargo run -p vectorshell-server -- --help`
- Generate a client binary through the server entrypoint: `cargo run -p vectorshell-server -- --config config/config.toml generate-client --target linux-amd64`

### Web app
- Install deps: `npm install --prefix webapp`
- Start the Vite dev server: `npm run --prefix webapp dev`
- Build the web app: `npm run --prefix webapp build`
- Lint the web app: `npm run --prefix webapp lint`
- Preview the production web build: `npm run --prefix webapp preview`

## Configuration and runtime shape
- Main server config lives in `config/config.toml`; examples/test variants are in `config/config.example.toml` and `config/config.test.toml`.
- `server.src.config` deserializes five config areas: server listen/ws path, LLM agent settings, embedded client defaults, auth tokens, and optional TLS.
- The server can serve both the API and the built frontend on the same port; production frontend assets are expected in `webapp/dist` and are mounted at `/webapp`.
- Client binaries are parameterized at build time via compile-time env vars (`VECTOR_SERVER_URL`, `VECTOR_AUTH_TOKEN`, `VECTOR_BUILD_UUID`, `VECTOR_INSECURE_TLS`, `VECTOR_RECONNECT_INTERVAL`). The server’s `generate-client` path sets these before running `cargo build -p vectorshell-client --release`.
- Agent identity/preamble is loaded from `config/SOUL.md` when present, otherwise the built-in default prompt is used.

## High-level architecture

### Workspace structure
- `server/`: axum-based control plane and API.
- `client/`: remote client that connects back over WebSocket and executes server-issued jobs/tools.
- `shared/`: wire protocol types shared by server and client.
- `webapp/`: React/Vite frontend for session browsing, chat, tool activity, and artifact flows.

### Server architecture
- `server/src/main.rs` is the orchestration entrypoint. It loads config, constructs the `Agent`, `ClientManager`, SQLite `Db`, in-memory event bus, and UI state, then runs the API server and local REPL together.
- `server/src/api/mod.rs` is the main HTTP/WebSocket boundary. It handles:
  - client WebSocket registration and message handling
  - authenticated REST endpoints for sessions, conversations, artifacts, and client generation
  - SSE streams for per-session and per-conversation updates
  - static serving of `webapp/dist`
- `server/src/client_manager/mod.rs` is the in-memory live-session registry. It stores active connections, advertised client capabilities, pending tool/exec requests, and exec history.
- `server/src/db/mod.rs` is the persistent history layer in SQLite (`data/vectorshell.db`). It stores session presence, command history, chat history, and conversation-to-install mappings.
- `server/src/event_bus/mod.rs` is a per-key broadcast hub used to fan out SSE events. Session streams and conversation streams are keyed separately in-memory.
- `server/src/builder/mod.rs` wraps client compilation/copying into `build/clients/`.

### Agent/tool flow
- `server/src/agent/mod.rs` builds the LLM-facing agent and registers the available tools.
- Tool implementations are split by concern:
  - `server/src/agent/exec_tool.rs`
  - `server/src/agent/file_tools.rs`
  - `server/src/agent/windows_tools.rs`
- The server agent does not execute locally on the server target host; tools are marshaled through `ClientManager` to the selected connected client, then results are returned to the agent loop.
- The agent loop keeps a small in-memory list of prior tool outputs and has a repeated-call guard to stop the model from issuing the same tool call over and over.

### Client architecture
- `client/src/main.rs` is a thin wrapper around `websocket::run_client()`.
- `client/src/websocket/mod.rs` owns reconnect behavior, WebSocket registration, heartbeats, and dispatch of server messages.
- On connect, the client generates a fresh `connection_id`, reuses/persists an `install_id`, collects host metadata, advertises tool capabilities, and registers with the server.
- `client/src/executor.rs` is the execution backend for server-issued work. It implements shell exec, file read/write/upload/download, chunked download, and Windows-only CLR/.NET helpers.
- The server relies on the client’s advertised capability list before dispatching tools.

### Shared protocol
- `shared/src/protocol.rs` is the contract between server and client.
- Messages are tagged enums:
  - client → server: register, heartbeat, exec result, tool result
  - server → client: exec, upload/download placeholders, ping, tool call
- If you change any wire shape here, you almost certainly need coordinated server and client updates.

### Frontend architecture
- `webapp/src/App.tsx` is the main application and currently holds most UI state.
- The frontend talks only to the server HTTP API; it does not connect directly to clients.
- There are two SSE channels in the UI model:
  - session stream: operational events like exec/tool results for the selected connection
  - conversation stream: agent/tool lifecycle events for the active conversation
- Session selection, conversation IDs, API base URL, and auth token are cached in browser local storage.

## Important implementation relationships
- Session identity has multiple layers: `client_id`/`connection_id` identify a live connection, while `install_id` ties history across reconnects. Persistent history and conversations are keyed by `install_id`; live routing is keyed by the active connection.
- REST auth and client WebSocket auth are separate tokens from config.
- The API docs under `api-docs/` describe the intended external REST/SSE surface and are useful when changing frontend/server API behavior.
