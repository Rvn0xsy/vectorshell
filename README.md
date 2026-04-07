# VectorShell

[English](./README.md) | [中文](./README_zh.md)

An AI-driven remote command execution platform built with Rust. VectorShell bridges AI agents and remote targets — run shell commands, manage files, and execute platform-specific tools across your fleet, all orchestrated through a simple REPL, web UI, or MCP-compatible API.

## Features

- **AI-Powered**: Leverage LLMs to reason about remote environments and execute context-aware commands
- **Remote Execution**: Execute shell commands, read/write files, and transfer data between server and clients
- **MCP Compatible**: Expose tools to any MCP-compatible AI client (Claude Desktop, etc.) via built-in MCP server
- **Cross-Platform**: Clients for Linux, macOS, and Windows with system proxy support
- **Web UI**: Built-in dashboard for session management and real-time event monitoring
- **TLS Support**: Secure communication with `wss://` and certificate-based encryption

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        VectorShell Server                        │
│                                                                  │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌──────┐ │
│  │   REPL   │  │ REST API │  │  SSE    │  │  Web   │  │ MCP  │ │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘  │Server│ │
│                                                          └──────┘ │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                      AI Agent (LLM)                          │ │
│  └─────────────────────────────────────────────────────────────┘ │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                    Client Manager                            │ │
│  │         (session registry, tool dispatch, events)             │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
        │ WebSocket (reverse connect)
        ▼
┌─────────────────────────────────────────────────────────────┐
│                      VectorShell Client                      │
│                                                                  │
│  Shell Exec │ File Ops │ Upload/Download │ Windows Tools        │
└─────────────────────────────────────────────────────────────┘
```

## Prerequisites

- **Rust** 1.75+ (for building)
- **Node.js** 18+ (for frontend development)
- **OpenAI-compatible API** or **Claude API** (for AI agent)

## Quick Start

```bash
# Clone and build
cargo build --release

# Configure
cp config/config.example.toml config/config.toml
# Edit config/config.toml with your API keys and settings

# Run server
./target/release/vectorshell-server --config config/config.toml

# In another terminal: run a client on a target machine
./target/release/vectorshell-client
```

## Build

| Command | Description |
|---------|-------------|
| `make build` | Build release binaries (server + client) + frontend |
| `make build-release` | Build Rust release binaries only |
| `make build-server` | Build server only |
| `make build-client` | Build client only |
| `make test` | Run all Rust tests |
| `make web-dev` | Start Vite dev server for frontend |
| `make web-build` | Build frontend for production |
| `make lint` | Run `cargo fmt` && `cargo clippy` |
| `make clean` | Clean build artifacts |

## Configuration

Edit `config/config.toml`:

```toml
[server]
listen = "0.0.0.0:8080"
ws_path = "/ws"
ui_path = "/ui"
ui_dist = "dashboard/dist"

[agent]
model = "gpt-5.2-codex"
base_url = "https://api.openai.com/v1"
api_key = "your-api-key"

[auth]
api_token = "your-api-token"       # Bearer token for REST API
client_token = "your-client-token" # Token embedded in clients

[mcp]
enabled = true                     # Enable MCP server at /mcp
```

## Agent Preamble (SOUL.md)

The AI agent's behavior is defined by `config/SOUL.md` — a markdown preamble loaded at startup. If absent, a built-in default is used.

```bash
# Create from example
cp config/SOUL.example.md config/SOUL.md
# Edit to customize agent identity, tools, and response style
```

Key sections in SOUL.md:

| Section | Purpose |
|---------|---------|
| Identity | Agent name and role |
| Mission | Core objectives and priorities |
| Tool Semantics | Description of each available tool |
| Policy | Behavioral rules and restrictions |
| Chinese Mapping | Maps Chinese commands to tool calls |
| Response Style | Concise/factual guidance |

The file is gitignored — each deployment can have its own customized preamble without affecting the repo.

## MCP Server

VectorShell includes a built-in MCP server that exposes all tools to MCP-compatible AI clients.

### Endpoint

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/mcp` | JSON-RPC 2.0 requests |
| `GET` | `/mcp` | SSE keepalive stream |

### Authentication

Uses the `api_token` from config:

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer your-api-token" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```

### Available Tools

| Tool | Description |
|------|-------------|
| `exec` | Execute shell command |
| `read_file` | Read file contents |
| `write_file` | Write file contents |
| `upload_file` | Upload file to client |
| `download_file` | Download file from client |
| `powershell_clr` | Execute PowerShell (Windows) |
| `dotnet_assembly` | Execute .NET assembly (Windows) |

### Usage Example

```bash
# List available tools
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer your-api-token" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# Execute a command (get install_id from /api/sessions first)
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer your-api-token" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
      "name": "exec",
      "arguments": {
        "install_id": "abc123",
        "command": "uname -a"
      }
    }
  }'
```

## Server REPL

After starting the server, interact via REPL:

| Command | Description |
|---------|-------------|
| `/sessions` | List connected clients |
| `/use <install_id>` | Select a client |
| `/info` | Show selected client details |
| `/exec <cmd>` | Execute command |
| `/read <path>` | Read file |
| `/write <path> <content>` | Write file |
| `/upload <src> <dst>` | Upload file to client |
| `/download <src> <dst>` | Download file from client |
| `/agent <prompt>` | Ask AI agent |
| `/tool <name> <json>` | Call tool by name |
| `/clear` | Clear context |
| `/back` | Unselect client |
| `/help` | Show all commands |

## Client Binary Generation

Generate pre-configured client binaries for deployment:

```bash
# Generate for current platform
./target/release/vectorshell-server --config config/config.toml generate-client

# Cross-compile for other platforms
./target/release/vectorshell-server --config config/config.toml generate-client --target linux-arm64
```

Supported targets: `linux-amd64`, `linux-arm64`, `windows-amd64`, `windows-arm64`, `macos-amd64`, `macos-arm64`

Output: `build/clients/vectorshell-client`

## Frontend

The web dashboard is served at `/ui` when configured:

```
http://localhost:8080/ui
```

For local frontend development:

```bash
make web-dev
# Frontend: http://localhost:5173
# Backend: http://localhost:8080
```

## TLS

Enable TLS in `config.toml`:

```toml
[tls]
enabled = true
cert_path = "config/certs/cert.pem"
key_path = "config/certs/key.pem"
```

For self-signed certificates, set `client.insecure_tls = true` before generating clients.

## Windows Proxy

Windows clients automatically detect system proxy:

1. WinHTTP auto-proxy (PAC/WPAD)
2. Manual proxy settings

The client establishes an HTTP CONNECT tunnel when proxy is detected.

## Security

- **API Token**: Protect with network access controls or use TLS
- **Client Token**: Embedded in clients; use TLS in production
- **No Built-in Auth**: Rely on network isolation and TLS for access control

## Project Structure

```
vectorshell/
├── server/           # axum API server, AI agent, client manager
├── client/          # Reverse-connect WebSocket client
├── shared/          # Wire protocol types
├── dashboard/       # React/Vite frontend
├── config/         # Configuration files
└── docs/           # Project documentation (API, dev, plans, specs)
```

## License

MIT
