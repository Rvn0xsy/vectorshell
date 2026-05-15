# VectorShell

[English](./README.md) | [中文](./README_zh.md)

Go-based remote command execution platform with AI agent orchestration. Reverse WebSocket clients, DPI bypass tunnel, SSE streams, MCP endpoint, and React dashboard.

## Features

- AI-driven remote tool orchestration (exec, file read/write, upload, download)
- Reverse WebSocket client with auto-reconnect
- **DPI bypass tunnel** — AES-256-CTR encrypted tunnel with TCP fragmentation through CONNECT proxy
- Built-in client cross-compilation and download API
- MCP JSON-RPC endpoint at `/mcp`
- React/Vite dashboard at `/ui`
- SQLite-backed conversation and artifact persistence

## Repository Layout

```
cmd/server          Server entrypoint
cmd/client          Client entrypoint
cmd/repl            Local REPL entrypoint
internal/api        HTTP API, SSE, WebSocket, MCP
internal/agent      Eino-based AI agent service
internal/client     Client runtime and tool executor
internal/config     TOML config loading
internal/embedded   Build-time ldflags injection
internal/events     In-memory SSE broadcast bus
internal/mcp        MCP JSON-RPC types
internal/protocol   WebSocket message envelope
internal/session    Session registry and tool dispatch
internal/store      SQLite persistence
internal/tunnel     DPI bypass: FragmentingConn + EncryptedConn
dashboard           React/Vite frontend
skills              Skill documents consumed by the agent
```

## Quick Start

```bash
cp config.example.toml config.toml
# edit config.toml with your API key

go run ./cmd/server -config ./config.toml
```

In another terminal:

```bash
go run ./cmd/client -config ./config.toml
```

For the local REPL:

```bash
go run ./cmd/repl -config ./config.toml
```

## Configuration

See `config.example.toml` for all options. Key sections:

```toml
[server]
listen = ":8080"
public_url = "wss://your-domain.com/ws"   # external URL for generated clients

[agent]
model = "gpt-4.1"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."

[tunnel]
enabled = true
pre_shared_key = "your-32-byte-key-here-!!!!!!!!"
port = 7735
host = "your-server-ip-or-domain"
proxy_host = "proxy-ip"
proxy_port = 8002
```

## DPI Bypass Tunnel

When the client is behind a restrictive CONNECT proxy that performs SSL DPI:

1. Client dials the CONNECT proxy directly
2. Sends `CONNECT <tunnel_host>:<tunnel_port>` through the proxy
3. Wraps connection with 1-byte TCP fragmentation (defeats DPI reassembly)
4. AES-256-CTR encryption over the tunnel
5. WebSocket runs over the encrypted tunnel — no TLS needed

Generated clients with `tunnel.enabled = true` embed all tunnel settings. No config.toml needed on the client side.

## Build Commands

| Command | Description |
|---------|-------------|
| `make build` | Build server and client binaries |
| `make build-server` | Build the server binary |
| `make build-client` | Build the client binary |
| `make test` | Run Go tests |
| `make web-dev` | Start the dashboard dev server |
| `make web-build` | Build the dashboard |
| `make docker-build` | Build the Docker image |

## API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| GET | `/api/sessions` | List connected sessions |
| GET | `/api/sessions/{id}/events` | SSE stream for session |
| GET | `/api/sessions/{id}/history` | Conversation history |
| POST | `/api/sessions/{id}/tools` | Dispatch tool to session |
| POST | `/api/sessions/{id}/clean` | Clear conversation |
| POST | `/api/conversations` | Create conversation |
| POST | `/api/conversations/{id}/messages` | Send message (async, SSE) |
| GET | `/api/conversations/{id}/events` | SSE stream for conversation |
| POST | `/api/artifacts` | Upload file artifact |
| GET | `/api/artifacts/{id}/download` | Download artifact |
| POST | `/api/clients/generate` | Cross-compile client binary |
| GET | `/api/clients/download` | Download pre-built client |
| GET/POST | `/mcp` | MCP endpoint |

All endpoints except `/api/health` and WebSocket require `Authorization: Bearer <api_token>`.

## REPL Commands

```
/sessions                   List connected clients
/use <install_id>           Select a client
/exec <cmd>                 Execute command
/tool <name> <json>         Call a remote tool
/agent <prompt>             Chat with the AI agent
/back                       Deselect client
/quit                       Exit
```

## Dashboard

```bash
make web-dev                  # Dev server at http://localhost:5173
make web-build                # Production build
```

## License

MIT
