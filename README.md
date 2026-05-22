[English](./README.md) | [中文](./README_zh.md)

<p align="center">
  <img src="https://img.shields.io/badge/Go-1.25+-00ADD8?style=flat&logo=go" alt="Go version">
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/platform-linux%20|%20darwin%20|%20windows-lightgrey" alt="Platform">
</p>

---

VectorShell is a Go-based remote operations platform that combines reverse-shell infrastructure, AI agent orchestration, and DPI bypass networking into a single cohesive system. Clients behind restrictive proxies connect through encrypted, fragmented tunnels; operators control them through a REST API, SSE event streams, MCP-compatible tools, or a React dashboard.

## Architecture

```
                          ┌──────────────────────────────────────────────┐
                          │                  OPERATOR                    │
                          │   Dashboard / API / REPL / MCP Client        │
                          └─────┬──────────┬──────────┬─────────────────┘
                                │ HTTPS    │ SSE      │ MCP (JSON-RPC)
                                ▼          ▼          ▼
              ┌─────────────────────────────────────────────────────────┐
              │                     VECTORSHELL SERVER                   │
              │                                                         │
     TLS ──── │  :5443 ─── nginx ──► :8084  HTTP API                    │
              │                    ┌─ /api/sessions, /api/conversations  │
              │                    │─ /api/clients/generate              │
              │                    │─ /api/artifacts, /api/agent         │
              │  :7735 ─── Tunnel  │─ /mcp  (JSON-RPC)                  │
              │     Listener ──────┤─ /ws   (WebSocket)                 │
              │                    │                                     │
              │  ┌──────────┐  ┌──┴──────────┐  ┌──────────────────┐   │
              │  │  Session │  │  Eino Agent  │  │  SQLite Store    │   │
              │  │  Manager │  │  (OpenAI)    │  │  + Events Bus    │   │
              │  └──────────┘  └──────────────┘  └──────────────────┘   │
              └─────────────────────────────────────────────────────────┘
                                           │
                          ┌────────────────┼────────────────┐
                          │                │                 │
                    Reverse WS      DPI Bypass Tunnel   Reverse WS
                          │                │                 │
              ┌───────────┴──────────┐     │     ┌───────────┴──────────┐
              │   CLIENT A           │     │     │   CLIENT B (Tunnel)   │
              │   (direct)           │     │     │                       │
              └──────────────────────┘     │     └───────────────────────┘
                                           │
                                    ┌──────┴──────┐
                                    │ CONNECT     │
                                    │ Proxy       │
                                    └─────────────┘
```

## DPI Bypass Tunnel

Clients behind restrictive CONNECT proxies that perform SSL DPI inspection connect through a multi-layer tunnel:

```
  Client                          CONNECT Proxy                     Server
    │                                  │                               │
    │── TCP dial ────────────────────► │                               │
    │                                  │                               │
    │── CONNECT host:7735 HTTP/1.1 ──► │ ──────────────────────────►  │
    │                                  │                               │
    │◄── 200 Connection Established ── │ ◄─────────────────────────    │
    │                                  │                               │
    │══╡ FragmentingConn (1-byte TCP segments) ╞══════════════════►  │
    │   │                              │            │                  │
    │   │  Each byte is a separate     │            │  DPI cannot      │
    │   │  TCP packet → DPI cannot     │            │  reassemble      │
    │   │  reassemble the stream       │            │  the stream      │
    │   │                              │            │                  │
    │══╡ EncryptedConn (AES-256-CTR)  ╞═══════════════════════════►  │
    │   │                              │            │                  │
    │   │  Random IV exchange per      │            │  Payload is      │
    │   │  direction → encrypted       │            │  opaque to DPI   │
    │   │  payload with no plaintext   │            │                  │
    │   │                              │            │                  │
    │══╡ WebSocket (ws://, no TLS)    ╞═══════════════════════════►  │
    │   │                              │            │                  │
    │   │  gorilla/websocket uses      │            │  No TLS needed:  │
    │   │  NetDialContext → plain TCP  │            │  encryption is   │
    │   │  (no double-TLS overhead)    │            │  at the tunnel   │
```

**Why it works:** DPI systems rely on TCP stream reassembly to inspect TLS handshakes. By fragmenting every write into 1-byte segments with no delay, the DPI box never accumulates enough data to classify the flow. The AES-256-CTR layer then encrypts the payload so even if a DPI box reassembles fragments, the content is opaque.

## Project Advantages

| Capability | Description |
|---|---|
| **Single binary** | Server and client are standalone Go binaries with zero runtime dependencies |
| **Reverse connections** | Clients initiate outbound connections; no inbound firewall rules needed |
| **DPI resistant** | TCP fragmentation + AES-256-CTR tunnel defeats deep packet inspection |
| **AI-native** | Eino-powered agent with exec, read_file, write_file, upload_file, download_file remote tools |
| **MCP compatible** | `/mcp` endpoint implements JSON-RPC 2.0; integrate with any MCP client (Claude Desktop, Continue, etc.) |
| **Real-time streaming** | SSE event streams deliver agent reasoning, tool calls, and results as they happen |
| **Persistent history** | SQLite-backed conversations survive server restarts |
| **Cross-compilation API** | `POST /api/clients/generate` builds platform-specific binaries with embedded config |
| **Skill documents** | Eino skill middleware loads markdown files from configurable directory into agent context |
| **Observable** | Health checks, session listing, event streaming — all over the same HTTP API |

## Quick Start

### Prerequisites

- Go 1.21+
- An OpenAI-compatible API key (or any provider with `/v1/chat/completions`)

### 1. Configure

```bash
cp config.example.toml config.toml
```

Edit `config.toml` — at minimum, set your API key:

```toml
[agent]
model = "gpt-4.1"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."

[auth]
api_token = "your-strong-api-token"
client_token = "your-strong-client-token"
```

### 2. Start the server

```bash
go run ./cmd/server -config ./config.toml
# vectorshell server listening on :8080
```

### 3. Connect a client

On the target machine:

```bash
go run ./cmd/client -config ./config.toml
```

Or generate a pre-configured binary from the server:

```bash
curl -X POST http://localhost:8080/api/clients/generate \
  -H "Authorization: Bearer your-strong-api-token" \
  -d '{"target": "windows-amd64"}'
```

### 4. Interact

**REPL** (local interactive shell):
```bash
go run ./cmd/repl -config ./config.toml
> /sessions
> /use <install_id>
> /exec whoami
> /agent "list all PDF files modified this week"
```

**REST API**:
```bash
curl http://localhost:8080/api/sessions -H "Authorization: Bearer your-token"
curl -X POST http://localhost:8080/api/agent \
  -H "Authorization: Bearer your-token" \
  -d '{"install_id":"...","prompt":"find large log files"}'
```

**MCP**: Point your MCP client at `http://localhost:8080/mcp` with the auth token.

### 5. Dashboard

```bash
make web-dev    # http://localhost:5173
```

## Configuration Reference

```toml
[server]
listen = ":8080"                    # HTTP listen address
ws_path = "/ws"                     # WebSocket endpoint path
public_url = ""                     # External URL for generated clients (e.g., wss://your-domain.com/ws)

[agent]
model = "gpt-4.1"                   # Model name
base_url = "https://api.openai.com/v1"
api_key = ""                        # Also reads OPENAI_API_KEY env var
soul_path = "SOUL.md"               # Base system prompt file

[skill]
enabled = true                      # Load skill documents into agent context
dir = "skill"                       # Directory containing skill markdown files

[auth]
api_token = ""                      # Bearer token for HTTP API
client_token = ""                   # Token clients must present on WebSocket registration

[client]
server_url = "ws://127.0.0.1:8080/ws"
reconnect_interval = 5              # Seconds between reconnect attempts

[store]
db_path = "data/vectorshell-go.db"  # SQLite database path

[tunnel]
enabled = false                     # Enable DPI bypass tunnel listener
pre_shared_key = ""                 # 32-byte AES key (must match client)
port = 7734                         # Tunnel listener port
host = ""                           # Public hostname for tunnel endpoint (used in generated clients)
proxy_host = ""                     # CONNECT proxy IP for clients behind a proxy
proxy_port = 0                      # CONNECT proxy port
```

## API Reference

All endpoints except `/api/health` and `/ws` require `Authorization: Bearer <api_token>`.

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/health` | Health check (no auth) |
| `GET` | `/api/sessions` | List connected clients with metadata |
| `GET` | `/api/sessions/:id/events` | SSE stream: tool dispatch, results, errors |
| `GET` | `/api/sessions/:id/history` | Conversation history for session |
| `POST` | `/api/sessions/:id/tools` | Dispatch a tool call to the client |
| `POST` | `/api/sessions/:id/clean` | Clear conversation context |

### Conversations

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/conversations` | Create conversation bound to install_id |
| `POST` | `/api/conversations/:id/messages` | Send user prompt → agent runs async, results over SSE |
| `GET` | `/api/conversations/:id/events` | SSE stream: agent reasoning, tool calls, final answer |

### Agent & Tools

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/agent` | Synchronous agent invocation |
| `POST` | `/api/tools` | Direct tool dispatch to client |

### Artifacts

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/artifacts` | Upload file (multipart, field `file`) |
| `GET` | `/api/artifacts/:id/download` | Download artifact by ID |

### Client Build

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/clients/generate` | Cross-compile client binary with embedded config |
| `GET` | `/api/clients/download?target=linux-amd64` | Download previously-built binary |

### MCP

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/mcp` | SSE keepalive for MCP transport |
| `POST` | `/mcp` | JSON-RPC: `initialize`, `tools/list`, `tools/call` |

### SSE Events

The SSE stream for conversations emits these event types:

| Event | Payload |
|-------|---------|
| `conversation.started` | `conversation_id`, `install_id`, `timestamp` |
| `tool.started` | `tool_name`, `args` |
| `tool.finished` | `tool_name`, `ok`, `data`, `duration_ms` |
| `agent.message` | `role`, `content`, `final` |
| `conversation.finished` | `conversation_id`, `ok` |
| `error` | `code`, `message` |

## REPL Commands

```
/sessions                    List connected clients
/use <install_id>            Select a client for subsequent commands
/exec <cmd>                  Run a shell command on the selected client
/tool <name> <json>          Dispatch a raw tool call
/agent <prompt>              Chat with the AI agent (manages tools automatically)
/back                        Deselect current client
/quit                        Exit
```

## Build

```bash
make build              # Server + client binaries
make build-server       # Server only
make build-client       # Client only
make test               # Run Go tests
make web-dev            # Dashboard dev server
make web-build          # Dashboard production build
make docker-build       # Docker image
```

## Deployment Example

Typical production setup with TLS termination:

```
Internet ──► nginx :5443 (TLS) ──► vectorshell :8084
                │                        │
                │ cert: acme.sh          │
                │ proxy_pass /ws         │
                └────────────────────────┘
```

```nginx
server {
    listen 5443 ssl;
    server_name your-domain.com;
    ssl_certificate     /path/to/fullchain.cer;
    ssl_certificate_key /path/to/private.key;

    location /ws {
        proxy_pass http://127.0.0.1:8084;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 86400s;
    }
    location / {
        proxy_pass http://127.0.0.1:8084;
    }
}
```

## License

MIT
