# VectorShell

VectorShell is an AI-driven remote command execution platform written in Rust.

- **Server** hosts the AI agent, tracks clients, and dispatches commands.
- **Client** connects back to server over WebSocket, executes commands, and returns results.
- **Shared** crate defines protocol types used by both sides.

## Quick Start

Build all crates:

```bash
cargo build
```

Run server:

```bash
./target/debug/vectorshell-server --config config/config.toml
```

Run local client:

```bash
./target/debug/vectorshell-client
```

## Generate Client Binary

Generate a client with embedded config values from `config/config.toml`:

```bash
./target/debug/vectorshell-server --config config/config.toml generate-client --target linux-amd64
```

Common targets:

- `linux-amd64`
- `linux-arm64`
- `windows-amd64`
- `windows-arm64`
- `macos-amd64`
- `macos-arm64`

Output path:

```text
build/clients/
```

## Embedded Client Config

Generated client embeds these values at compile time:

- `VECTOR_SERVER_URL`
- `VECTOR_AUTH_TOKEN`
- `VECTOR_RECONNECT_INTERVAL`
- `VECTOR_INSECURE_TLS`

This means changing `config.toml` requires regenerating client binaries.

## TLS / WSS Notes

- Server supports TLS (`wss://`) when `[tls].enabled = true` and certificate/key are configured.
- Client supports `wss://`.
- For self-signed cert environments, set `client.insecure_tls = true` in config **before** running `generate-client`.

## Windows Proxy Behavior

Generated Windows client supports system proxy discovery:

1. WinHTTP auto proxy (PAC/WPAD)
2. Internet Settings manual proxy (`ProxyServer`)

If a proxy is resolved, client uses HTTP CONNECT tunnel for WS/WSS.

## Server REPL Commands

- `clients`
- `use <client_id>`
- `exec <command>`
- `agent <task>`
- `agent-exec <task>`
