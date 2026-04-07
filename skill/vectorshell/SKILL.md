---
name: vectorshell
description: Use when working with VectorShell — either developing the codebase (building, testing, configuring, generating clients) or operating live sessions via the server API (listing sessions, sending messages, dispatching tools, handling artifacts). Triggers include building the project, running tests, understanding architecture, config.toml settings, Makefile commands, REPL usage, or any remote execution workflow through the API.
---

# VectorShell

## Project overview

VectorShell is an AI-driven remote command execution platform written in Rust.

- **Server** hosts the AI agent, REST API, WebSocket hub, and serves the frontend UI.
- **Client** connects back to server over reverse WebSocket, executes server-issued tools, returns results.
- **Shared** defines the wire protocol (JSON tagged enums over WebSocket).
- **Dashboard** is the React/Vite frontend.

## Two usage modes

### Development mode

Use `vectorshell` skill for development tasks: building, testing, configuring, generating client binaries, understanding architecture.

### Operations mode

Use `vectorshell` skill for operating live sessions via the REST API and SSE streams.

---

## Development reference

### Common commands

```bash
# Build everything (Rust release + frontend)
make build

# Build Rust only
cargo build --release

# Run server
make up

# Generate client binary (embeds config at compile time)
make gen-client
make gen-client TARGET=linux-arm64

# Test
cargo test
cargo test -p shared
cargo test -p shared register_message_roundtrip -- --exact

# Frontend dev
make web-dev      # Vite dev server (port 5173)
make web-build   # Production build

# Lint
make lint         # fmt + clippy
```

### Makefile targets

| Command | Description |
|---------|-------------|
| `make build` | Rust release + frontend production build |
| `make build-release` | Rust release only |
| `make build-server` | Server only |
| `make build-client` | Client only |
| `make test` | All Rust tests |
| `make up` | Run server with default config |
| `make web-dev` | Start Vite dev server |
| `make web-build` | Build frontend |
| `make gen-client` | Generate client binary |
| `make gen-client TARGET=x` | Cross-compile for target |
| `make lint` | Format + clippy |
| `make clean` | Clean artifacts |

### Configuration (`config/config.toml`)

```toml
[server]
listen = "0.0.0.0:8080"
ws_path = "/ws"
ui_path = "/ui"              # frontend URL path (default: /ui)
ui_dist = "dashboard/dist"   # frontend dist (omit to disable)

[agent]
model = "gpt-5.2-codex"
base_url = "https://api.openai.com/v1"
api_key = "..."

[client]
default_server = "wss://..."
reconnect_interval = 5
insecure_tls = false

[auth]
api_token = "..."     # REST API Bearer token
client_token = "..."  # Client WebSocket auth
```

Agent preamble: `config/SOUL.md` (loaded when present).

### Session identity model

| Term | Scope | Purpose |
|------|-------|---------|
| `connection_id` | live connection | Single WebSocket connection identifier |
| `session_id` | live routing | Server-assigned; used for live message routing |
| `install_id` | persistent | Stable across reconnects; all history keyed by this |

The REPL `/use <install_id>` maps to `session_id` via `ClientManager::get_by_install_id`.

### Architecture

Three crates:
- `server/` — axum API server, LLM agent (rig), WebSocket hub, SQLite (`data/vectorshell.db`), event bus
- `client/` — reverse-connect WS client; embeds config via `env!` constants
- `shared/` — wire protocol (`shared/src/protocol.rs`)

Agent/tool flow: server agent → `ClientManager` → selected client → result back. Server never executes tools locally.

### Cross-compilation targets

| Alias | Triple |
|-------|--------|
| `linux-amd64` | `x86_64-unknown-linux-gnu` |
| `linux-arm64` | `aarch64-unknown-linux-gnu` |
| `windows-amd64` | `x86_64-pc-windows-gnu` |
| `windows-arm64` | `aarch64-pc-windows-gnu` |
| `macos-amd64` | `x86_64-apple-darwin` |
| `macos-arm64` | `aarch64-apple-darwin` |

Embedded env vars: `VECTOR_SERVER_URL`, `VECTOR_AUTH_TOKEN`, `VECTOR_RECONNECT_INTERVAL`, `VECTOR_INSECURE_TLS`, `VECTOR_BUILD_UUID`. Config change → re-run `make gen-client`.

### Server REPL commands

| Command | Description |
|---------|-------------|
| `/sessions` | List connected clients |
| `/use <install_id>` | Select client (enters agent mode) |
| `/info` | Show selected session details |
| `/exec <cmd>` | Execute raw command on selected client |
| `/read <path>` | Read file |
| `/write <path> <content>` | Write file |
| `/upload <src> <dst>` | Upload server file to client |
| `/download <src> <dst>` | Download client file to server |
| `/tool <name> <json>` | Generic tool dispatch |
| `/agent <prompt>` | Ask AI for text response |
| `/clear` | Clear context history |
| `/back` | Exit agent mode |
| `/clean` | Clear session history + context |
| `/help` | Show all commands |

---

## Operations reference

### Required inputs

1. `server_base_url` (example: `https://host:8443`)
2. `api_token`
3. Task intent
4. Optional target selector (`install_id`, hostname, OS)

If target is ambiguous and multiple sessions exist, ask one concise clarification question.

### Operations workflow

#### 1) Session discovery

```
GET /api/health
GET /api/sessions
```

Pick target `install_id`.

#### 2) Conversation channel

```
POST /api/conversations   (body: {"install_id": "..."})
GET  /api/conversations/{conversation_id}/events  (SSE)
POST /api/conversations/{conversation_id}/messages
```

#### 3) Direct tool dispatch

```
POST /api/sessions/{install_id}/tools
body: {"tool_name": "...", "args": {...}, "timeout_ms": ...}
```

#### 4) Artifact workflow

```
POST /api/artifacts   → artifact_id
use artifact_id in tool args
GET  /api/artifacts/{artifact_id}/download
```

### Tool preference order

1. `read_file` / `write_file`
2. `upload_file` / `download_file`
3. `exec`
4. Windows-only: `powershell_clr`, `dotnet_assembly` (only if client capability exists)

For `.NET` payloads, prefer `artifact_id` over inline `content_base64`.

### Error handling

| Error | Action |
|-------|--------|
| `401 unauthorized` | Verify `Authorization: Bearer <api_token>` matches `auth.api_token` |
| `404 not_found` | Verify IDs exist; if conversation missing, recreate once |
| `409 capability_mismatch` | Check client capabilities; switch tool or session |
| `408/timeout` | Increase `timeout_ms`; verify client heartbeat is fresh |

### Safety rules

1. Never force unsupported tools on a client.
2. Never expose full binary/base64 payloads in logs/UI.
3. Sanitize shown args: `content_base64` → `<base64:N chars>`; truncate long strings.
4. Prefer low-risk/reversible actions before destructive ones.

### Output format

Always report:
1. Target (`install_id` or selection rule)
2. API calls performed
3. Tool calls (sanitized args)
4. Outcome/evidence
5. Next safe step

For full endpoint examples and SSE event reference, read:
- `references/api-endpoints.md`
- `references/sse-events.md`
