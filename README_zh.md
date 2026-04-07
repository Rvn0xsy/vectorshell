# VectorShell

[English](./README.md) | [中文](./README_zh.md)

使用 Rust 构建的 AI 驱动远程命令执行平台。VectorShell 桥接 AI Agent 与远程目标 — 通过简洁的 REPL、Web UI 或 MCP 兼容 API，在整个 fleet 中执行 shell 命令、管理文件和调用平台特定工具。

## 功能特性

- **AI 驱动**: 利用大语言模型对远程环境进行推理，执行上下文感知的命令
- **远程执行**: 在服务端与客户端之间执行 shell 命令、读写文件、传输数据
- **MCP 兼容**: 通过内置 MCP 服务端向任何 MCP 兼容的 AI 客户端（如 Claude Desktop）暴露工具
- **跨平台**: 支持 Linux、macOS 和 Windows 客户端，带系统代理支持
- **Web UI**: 内置仪表盘用于会话管理和实时事件监控
- **TLS 支持**: 通过 `wss://` 和证书加密实现安全通信

## 架构图

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
│  │         (会话注册表, 工具分发, 事件广播)                       │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
        │ WebSocket (反向连接)
        ▼
┌─────────────────────────────────────────────────────────────┐
│                      VectorShell Client                      │
│                                                                  │
│  Shell 执行 │ 文件操作 │ 上传/下载 │ Windows 工具               │
└─────────────────────────────────────────────────────────────┘
```

## 环境要求

- **Rust** 1.75+ (用于构建)
- **Node.js** 18+ (用于前端开发)
- **OpenAI 兼容 API** 或 **Claude API** (用于 AI Agent)

## 快速开始

```bash
# 克隆并构建
cargo build --release

# 配置
cp config/config.example.toml config/config.toml
# 编辑 config/config.toml，填写你的 API keys 和设置

# 运行服务端
./target/release/vectorshell-server --config config/config.toml

# 另一个终端：在目标机器上运行客户端
./target/release/vectorshell-client
```

## 构建命令

| 命令 | 说明 |
|------|------|
| `make build` | 构建 release 二进制 (server + client) + 前端 |
| `make build-release` | 仅构建 Rust release 二进制 |
| `make build-server` | 仅构建服务端 |
| `make build-client` | 仅构建客户端 |
| `make test` | 运行所有 Rust 测试 |
| `make web-dev` | 启动前端 Vite 开发服务器 |
| `make web-build` | 构建前端生产版本 |
| `make lint` | 运行 `cargo fmt` && `cargo clippy` |
| `make clean` | 清理构建产物 |

## 配置

编辑 `config/config.toml`:

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
api_token = "your-api-token"       # REST API Bearer token
client_token = "your-client-token" # 嵌入客户端的 token

[mcp]
enabled = true                     # 启用 /mcp MCP 服务端
```

## MCP 服务端

VectorShell 内置 MCP 服务端，向 MCP 兼容的 AI 客户端暴露所有工具。

### 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/mcp` | JSON-RPC 2.0 请求 |
| `GET` | `/mcp` | SSE 保活流 |

### 认证

使用配置中的 `api_token`:

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer your-api-token" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```

### 可用工具

| 工具 | 说明 |
|------|------|
| `exec` | 执行 shell 命令 |
| `read_file` | 读取文件内容 |
| `write_file` | 写入文件内容 |
| `upload_file` | 上传文件到客户端 |
| `download_file` | 从客户端下载文件 |
| `powershell_clr` | 执行 PowerShell (Windows) |
| `dotnet_assembly` | 执行 .NET 程序集 (Windows) |

### 使用示例

```bash
# 列出可用工具
curl -X POST http://localhost:8080/mcp \
  -H "Authorization: Bearer your-api-token" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# 执行命令 (先从 /api/sessions 获取 install_id)
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

## 服务端 REPL

启动服务端后，通过 REPL 进行交互：

| 命令 | 说明 |
|------|------|
| `/sessions` | 列出已连接客户端 |
| `/use <install_id>` | 选择客户端 |
| `/info` | 显示已选客户端详情 |
| `/exec <cmd>` | 执行命令 |
| `/read <path>` | 读取文件 |
| `/write <path> <content>` | 写入文件 |
| `/upload <src> <dst>` | 上传文件到客户端 |
| `/download <src> <dst>` | 从客户端下载文件 |
| `/agent <prompt>` | 向 AI Agent 提问 |
| `/tool <name> <json>` | 按名称调用工具 |
| `/clear` | 清除上下文 |
| `/back` | 取消选择客户端 |
| `/help` | 显示所有命令 |

## 客户端二进制生成

生成预配置的客户端二进制用于部署：

```bash
# 为当前平台生成
./target/release/vectorshell-server --config config/config.toml generate-client

# 交叉编译为其他平台
./target/release/vectorshell-server --config config/config.toml generate-client --target linux-arm64
```

支持的目标平台: `linux-amd64`, `linux-arm64`, `windows-amd64`, `windows-arm64`, `macos-amd64`, `macos-arm64`

输出: `build/clients/vectorshell-client`

## 前端

配置后，Web 仪表盘在 `/ui` 提供访问:

```
http://localhost:8080/ui
```

本地前端开发:

```bash
make web-dev
# 前端: http://localhost:5173
# 后端: http://localhost:8080
```

## TLS

在 `config.toml` 中启用 TLS:

```toml
[tls]
enabled = true
cert_path = "config/certs/cert.pem"
key_path = "config/certs/key.pem"
```

使用自签名证书时，在生成客户端前设置 `client.insecure_tls = true`。

## Windows 代理

Windows 客户端自动检测系统代理:

1. WinHTTP 自动代理 (PAC/WPAD)
2. 手动代理设置

检测到代理时，客户端会建立 HTTP CONNECT 隧道。

## 安全说明

- **API Token**: 通过网络访问控制或 TLS 进行保护
- **Client Token**: 嵌入客户端中；生产环境请使用 TLS
- **无内置认证**: 依赖网络隔离和 TLS 进行访问控制

## 项目结构

```
vectorshell/
├── server/           # axum API 服务端, AI Agent, 客户端管理器
├── client/           # 反向连接 WebSocket 客户端
├── shared/           # 服务端与客户端共用的协议类型
├── dashboard/        # React/Vite 前端
├── config/           # 配置文件
└── docs/             # 项目文档 (API, 开发, 计划, 设计)
```

## License

MIT
