# TASKS.md

## Project

VectorShell

This document defines implementation tasks for building VectorShell.

The project consists of:

* VectorShell Server
* VectorShell Client
* Shared Protocol

---

# Development Rules

AI agents implementing this repository must:

1. Implement tasks sequentially
2. Ensure compilation after each phase
3. Avoid unnecessary abstractions
4. Prefer simple implementations

Each phase must produce a runnable binary.

---

# Phase 1 — Workspace Setup

Create Rust workspace:

```
vectorshell
│
├── Cargo.toml
├── server
├── client
└── shared
```

Workspace configuration:

```
[workspace]
members = [
  "server",
  "client",
  "shared"
]
```

---

# Phase 2 — Shared Protocol

Create shared protocol definitions.

File:

```
shared/src/protocol.rs
```

Define message structures:

```
RegisterMessage
ExecMessage
ResultMessage
HeartbeatMessage
```

Use:

```
serde
serde_json
```

Add serialization tests.

---

# Phase 3 — Server Configuration

Implement configuration loader.

File:

```
server/src/config/mod.rs
```

Load configuration from:

```
config/config.toml
```

Use:

```
serde
toml
```

Verify configuration loads successfully.

---

# Phase 4 — WebSocket Server

Implement WebSocket server.

Directory:

```
server/src/websocket
```

Use:

```
tokio
tokio-tungstenite
```

Server must:

```
accept connections
handle messages
register clients
```

---

# Phase 5 — Client Manager

Create module:

```
server/src/client_manager
```

Responsibilities:

```
track connected clients
store client metadata
route messages
```

Use:

```
HashMap<ClientID, Connection>
```

---

# Phase 6 — Reverse WebSocket Client

Implement client connection.

File:

```
client/src/websocket/mod.rs
```

Client must:

```
connect to server
send register message
maintain heartbeat
reconnect on failure
```

Client must read configuration from **embedded constants**.

---

# Phase 7 — Command Execution

Implement remote command execution.

File:

```
client/src/executor.rs
```

Use:

```
tokio::process::Command
```

Client must:

```
receive exec message
execute command
return result
```

---

# Phase 8 — Server CLI

Add server REPL interface.

Commands:

```
clients
use <client_id>
exec <command>
```

The server must be able to dispatch commands to a selected client.

---

# Phase 9 — Client Builder

Implement client builder module.

Directory:

```
server/src/builder
```

Command:

```
vectorshell-server generate-client
```

Responsibilities:

```
compile client
embed server config
embed auth token
produce client binary
```

Implementation method:

```
cargo build -p client
```

With environment variables:

```
VECTOR_SERVER_URL
VECTOR_AUTH_TOKEN
VECTOR_RECONNECT_INTERVAL
VECTOR_INSECURE_TLS
```

Output binaries to:

```
build/clients/
```

---

# Phase 10 — Embedded Client Config

Client must define constants:

```
SERVER_URL
AUTH_TOKEN
RECONNECT_INTERVAL_SECS
INSECURE_TLS_RAW
```

Using:

```
env! macro
```

Example:

```
const SERVER_URL: &str = env!("VECTOR_SERVER_URL");
```

Verify client starts without arguments.

Note: these values are compile-time embedded. Any config change requires re-running `generate-client`.

---

# Phase 11 — AI Agent Integration

Add AI agent module.

Directory:

```
server/src/agent
```

Use:

```
rig
```

Responsibilities:

```
analyze tasks
generate commands
interpret results
```

Implement agent loop.

---

# Phase 12 — Logging

Add structured logging.

Use:

```
tracing
```

Log:

```
client connections
commands executed
errors
```

---

# Phase 13 — MVP Completion

The MVP is complete when:

```
client connects automatically
server lists clients
server sends commands
client executes commands
results return successfully
AI agent generates commands
```

All binaries must compile and run successfully.

---

# Future Tasks

Optional features:

```
file transfer
task persistence
vector memory
web dashboard
multi-agent system
TUI interface
```
