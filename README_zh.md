<p align="center">
  <img src="https://img.shields.io/badge/Go-1.25+-00ADD8?style=flat&logo=go" alt="Go version">
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/platform-linux%20|%20darwin%20|%20windows-lightgrey" alt="Platform">
</p>

---

VectorShell 是一个 Go 实现的远程操作平台，将反向 Shell 基础设施、AI Agent 编排和 DPI 绕过网络层整合为一个统一的系统。在受限代理后方的客户端通过加密分片隧道连接；操作员通过 REST API、SSE 事件流、MCP 兼容工具或 React 仪表盘进行控制。

## 系统架构

```
                          ┌──────────────────────────────────────────────┐
                          │                  操作员                      │
                          │   Dashboard / API / REPL / MCP Client        │
                          └─────┬──────────┬──────────┬─────────────────┘
                                │ HTTPS    │ SSE      │ MCP (JSON-RPC)
                                ▼          ▼          ▼
              ┌─────────────────────────────────────────────────────────┐
              │                     VECTORSHELL 服务端                    │
              │                                                         │
     TLS ──── │  :5443 ─── nginx ──► :8084  HTTP API                    │
              │                    ┌─ /api/sessions, /api/conversations  │
              │                    │─ /api/clients/generate              │
              │                    │─ /api/artifacts, /api/agent         │
              │  :7735 ─── Tunnel  │─ /mcp  (JSON-RPC)                  │
              │     Listener ──────┤─ /ws   (WebSocket)                 │
              │                    │                                     │
              │  ┌──────────┐  ┌──┴──────────┐  ┌──────────────────┐   │
              │  │  Session │  │  Eino Agent  │  │  SQLite 存储     │   │
              │  │  Manager │  │  (OpenAI)    │  │  + 事件总线      │   │
              │  └──────────┘  └──────────────┘  └──────────────────┘   │
              └─────────────────────────────────────────────────────────┘
                                           │
                          ┌────────────────┼────────────────┐
                          │                │                 │
                    Reverse WS      DPI 绕过隧道       Reverse WS
                          │                │                 │
              ┌───────────┴──────────┐     │     ┌───────────┴──────────┐
              │   客户端 A            │     │     │   客户端 B (隧道)     │
              │   (直连)             │     │     │                       │
              └──────────────────────┘     │     └───────────────────────┘
                                           │
                                    ┌──────┴──────┐
                                    │ CONNECT     │
                                    │ 代理        │
                                    └─────────────┘
```

## DPI 绕过隧道

当客户端位于执行 SSL DPI 检测的 CONNECT 代理后方时，通过多层隧道连接：

```
  客户端                            CONNECT 代理                       服务端
    │                                  │                               │
    │── TCP 直连 ────────────────────► │                               │
    │                                  │                               │
    │── CONNECT host:7735 HTTP/1.1 ──► │ ──────────────────────────►  │
    │                                  │                               │
    │◄── 200 Connection Established ── │ ◄─────────────────────────    │
    │                                  │                               │
    │══╡ FragmentingConn (1字节 TCP 分片) ╞═══════════════════════►  │
    │   │                              │            │                  │
    │   │  每个字节独立 TCP 包发送      │            │  DPI 无法重组    │
    │   │  → DPI 无法重组数据流        │            │  完整数据流      │
    │   │                              │            │                  │
    │══╡ EncryptedConn (AES-256-CTR)  ╞═══════════════════════════►  │
    │   │                              │            │                  │
    │   │  双向随机 IV 交换            │            │  载荷对 DPI      │
    │   │  → 无明文特征                │            │  完全不可见      │
    │   │                              │            │                  │
    │══╡ WebSocket (ws://, 无 TLS)    ╞═══════════════════════════►  │
    │   │                              │            │                  │
    │   │  gorilla/websocket 使用      │            │  无需 TLS：      │
    │   │  NetDialContext → 纯 TCP     │            │  加密在隧道层    │
    │   │  (无双重 TLS 开销)           │            │                  │
```

**原理：** DPI 系统依赖 TCP 流重组来检测 TLS 握手。将每次写入拆分为 1 字节的独立 TCP 段发送后，DPI 设备永远无法积累足够数据来分类流量。AES-256-CTR 加密层确保即便 DPI 设备重组了分片数据，内容也是不透明的密文。

## 项目优势

| 能力 | 说明 |
|---|---|
| **单一二进制** | 服务端和客户端均为独立 Go 二进制文件，零运行时依赖 |
| **反向连接** | 客户端主动发起出站连接，无需配置入站防火墙规则 |
| **DPI 对抗** | TCP 分片 + AES-256-CTR 加密隧道绕过深度包检测 |
| **AI 原生** | Eino 驱动的 Agent，内置 exec、read_file、write_file、upload_file、download_file 远程工具 |
| **MCP 兼容** | `/mcp` 端点实现 JSON-RPC 2.0，可接入任何 MCP 客户端（Claude Desktop、Continue 等） |
| **实时流式** | SSE 事件流实时推送 Agent 推理过程、工具调用和返回结果 |
| **持久化历史** | SQLite 存储对话记录，服务重启不丢失 |
| **交叉编译 API** | `POST /api/clients/generate` 按需生成内嵌配置的平台二进制文件 |
| **技能文档** | Eino skill 中间件从可配置目录加载 markdown 文件注入 Agent 上下文 |
| **可观测性** | 健康检查、会话列表、事件流 — 全部通过同一 HTTP API 暴露 |

## 快速入门

### 环境要求

- Go 1.21+
- OpenAI 兼容的 API key（任何提供 `/v1/chat/completions` 的供应商均可）

### 1. 配置

```bash
cp config.example.toml config.toml
```

编辑 `config.toml`，至少填入 API key：

```toml
[agent]
model = "gpt-4.1"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."

[auth]
api_token = "your-strong-api-token"
client_token = "your-strong-client-token"
```

### 2. 启动服务端

```bash
go run ./cmd/server -config ./config.toml
# vectorshell server listening on :8080
```

### 3. 连接客户端

在目标机器上：

```bash
go run ./cmd/client -config ./config.toml
```

或从服务端生成预配置的二进制文件：

```bash
curl -X POST http://localhost:8080/api/clients/generate \
  -H "Authorization: Bearer your-strong-api-token" \
  -d '{"target": "windows-amd64"}'
```

### 4. 交互

**REPL**（本地交互式命令行）：
```bash
go run ./cmd/repl -config ./config.toml
> /sessions
> /use <install_id>
> /exec whoami
> /agent "列出本周修改过的所有 PDF 文件"
```

**REST API**：
```bash
curl http://localhost:8080/api/sessions -H "Authorization: Bearer your-token"
curl -X POST http://localhost:8080/api/agent \
  -H "Authorization: Bearer your-token" \
  -d '{"install_id":"...","prompt":"查找大日志文件"}'
```

**MCP**：将 MCP 客户端指向 `http://localhost:8080/mcp` 并携带认证 token。

### 5. 仪表盘

```bash
make web-dev    # http://localhost:5173
```

## 配置参考

```toml
[server]
listen = ":8080"                    # HTTP 监听地址
ws_path = "/ws"                     # WebSocket 端点路径
public_url = ""                     # 生成客户端使用的外部地址（如 wss://your-domain.com/ws）

[agent]
model = "gpt-4.1"                   # 模型名称
base_url = "https://api.openai.com/v1"
api_key = ""                        # 也支持 OPENAI_API_KEY 环境变量
soul_path = "SOUL.md"               # 基础系统提示词文件

[skill]
enabled = true                      # 加载技能文档到 Agent 上下文
dir = "skill"                       # 技能 markdown 文件目录

[auth]
api_token = ""                      # HTTP API 的 Bearer token
client_token = ""                   # 客户端 WebSocket 注册 token

[client]
server_url = "ws://127.0.0.1:8080/ws"
reconnect_interval = 5              # 重连间隔（秒）

[store]
db_path = "data/vectorshell-go.db"  # SQLite 数据库路径

[tunnel]
enabled = false                     # 启用 DPI 绕过隧道监听
pre_shared_key = ""                 # 32 字节 AES 密钥（客户端需一致）
port = 7734                         # 隧道监听端口
host = ""                           # 隧道端点的公开主机名（用于生成客户端）
proxy_host = ""                     # 客户端所在网络的 CONNECT 代理 IP
proxy_port = 0                      # CONNECT 代理端口
```

## API 参考

除 `/api/health` 和 `/ws` 外，所有端点均需 `Authorization: Bearer <api_token>`。

### 会话

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/health` | 健康检查（无需认证） |
| `GET` | `/api/sessions` | 列出已连接客户端及元数据 |
| `GET` | `/api/sessions/:id/events` | SSE 流：工具调度、结果、错误 |
| `GET` | `/api/sessions/:id/history` | 会话对话历史 |
| `POST` | `/api/sessions/:id/tools` | 向客户端发送工具调用 |
| `POST` | `/api/sessions/:id/clean` | 清空对话上下文 |

### 对话

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/conversations` | 创建绑定到 install_id 的对话 |
| `POST` | `/api/conversations/:id/messages` | 发送用户提示 → Agent 异步执行，结果通过 SSE 推送 |
| `GET` | `/api/conversations/:id/events` | SSE 流：Agent 推理、工具调用、最终答案 |

### Agent 与工具

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/agent` | 同步 Agent 调用 |
| `POST` | `/api/tools` | 直接向客户端发送工具调用 |

### Artifact

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/artifacts` | 上传文件（multipart，字段 `file`） |
| `GET` | `/api/artifacts/:id/download` | 按 ID 下载 artifact |

### 客户端构建

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/clients/generate` | 交叉编译内嵌配置的客户端二进制文件 |
| `GET` | `/api/clients/download?target=linux-amd64` | 下载已构建的二进制文件 |

### MCP

| 方法 | 路径 | 说明 |
|------|------|-------------|
| `GET` | `/mcp` | SSE keepalive（MCP 传输层） |
| `POST` | `/mcp` | JSON-RPC：`initialize`、`tools/list`、`tools/call` |

### SSE 事件

对话 SSE 流包含以下事件类型：

| 事件 | 载荷 |
|------|------|
| `conversation.started` | `conversation_id`, `install_id`, `timestamp` |
| `tool.started` | `tool_name`, `args` |
| `tool.finished` | `tool_name`, `ok`, `data`, `duration_ms` |
| `agent.message` | `role`, `content`, `final` |
| `conversation.finished` | `conversation_id`, `ok` |
| `error` | `code`, `message` |

## REPL 命令

```
/sessions                    列出在线客户端
/use <install_id>            选择客户端（后续命令的默认目标）
/exec <cmd>                  在选中客户端上执行 Shell 命令
/tool <name> <json>          发送原始工具调用
/agent <prompt>              与 AI Agent 对话（自动管理工具调用）
/back                        取消选择当前客户端
/quit                        退出
```

## 构建

```bash
make build              # 构建服务端 + 客户端
make build-server       # 仅构建服务端
make build-client       # 仅构建客户端
make test               # 运行 Go 测试
make web-dev            # 仪表盘开发服务器
make web-build          # 仪表盘生产构建
make docker-build       # Docker 镜像
```

## 部署示例

生产环境 TLS 终止典型配置：

```
公网 ──► nginx :5443 (TLS) ──► vectorshell :8084
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
