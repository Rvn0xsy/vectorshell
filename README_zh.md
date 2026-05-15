# VectorShell

[English](./README.md) | [中文](./README_zh.md)

Go 实现的远程命令执行平台，集成 AI Agent 编排。支持反向 WebSocket 客户端、DPI 绕过隧道、SSE 事件流、MCP 端点、React 仪表盘。

## 能力

- AI 驱动的远程工具编排（exec、文件读写、上传、下载）
- 反向 WebSocket 客户端，自动重连
- **DPI 绕过隧道** — AES-256-CTR 加密 + TCP 分片，穿越 CONNECT 代理
- 内建客户端交叉编译与下载 API
- MCP JSON-RPC 端点 (`/mcp`)
- React/Vite 仪表盘 (`/ui`)
- SQLite 持久化对话和 artifact

## 目录结构

```
cmd/server          服务端入口
cmd/client          客户端入口
cmd/repl            本地 REPL 入口
internal/api        HTTP API、SSE、WebSocket、MCP
internal/agent      Eino AI Agent 服务
internal/client     客户端运行时代码
internal/config     TOML 配置加载
internal/embedded   编译期 ldflags 注入
internal/events     内存 SSE 广播
internal/mcp        MCP JSON-RPC 类型
internal/protocol   WebSocket 消息封装
internal/session    会话管理与工具分发
internal/store      SQLite 持久化
internal/tunnel     DPI 绕过：FragmentingConn + EncryptedConn
dashboard           React/Vite 前端
skills              Agent 技能文档
```

## 快速开始

```bash
cp config.example.toml config.toml
# 编辑 config.toml，填入 API key

go run ./cmd/server -config ./config.toml
```

另一个终端启动客户端：

```bash
go run ./cmd/client -config ./config.toml
```

本地 REPL：

```bash
go run ./cmd/repl -config ./config.toml
```

## 配置

参见 `config.example.toml`。主要配置段：

```toml
[server]
listen = ":8080"
public_url = "wss://your-domain.com/ws"   # 生成客户端使用的外部地址

[agent]
model = "gpt-4.1"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."

[tunnel]
enabled = true
pre_shared_key = "your-32-byte-key-here-!!!!!!!!"
port = 7735
host = "服务器IP或域名"
proxy_host = "代理IP"
proxy_port = 8002
```

## DPI 绕过隧道

客户端在受限制的 CONNECT 代理后方，代理会对 TLS 做 DPI 检测时：

1. 客户端直连 CONNECT 代理
2. 发送 `CONNECT <tunnel_host>:<tunnel_port>` 通过代理建立隧道
3. TCP 1 字节分片（绕过 DPI 重组检测）
4. AES-256-CTR 加密隧道流量
5. WebSocket 在加密隧道上运行 — 不需要额外 TLS

启用 `tunnel.enabled = true` 生成的客户端会内嵌所有隧道配置，客户端无需 config.toml。

## 构建命令

| 命令 | 说明 |
|------|------|
| `make build` | 构建服务端和客户端 |
| `make build-server` | 构建服务端 |
| `make build-client` | 构建客户端 |
| `make test` | 运行 Go 测试 |
| `make web-dev` | 启动前端开发服务器 |
| `make web-build` | 构建前端生产版本 |
| `make docker-build` | 构建 Docker 镜像 |

## 主要接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| GET | `/api/sessions` | 在线会话列表 |
| GET | `/api/sessions/{id}/events` | 会话 SSE 事件流 |
| GET | `/api/sessions/{id}/history` | 对话历史 |
| POST | `/api/sessions/{id}/tools` | 向会话发送工具调用 |
| POST | `/api/sessions/{id}/clean` | 清空对话 |
| POST | `/api/conversations` | 创建对话 |
| POST | `/api/conversations/{id}/messages` | 发送消息（异步，SSE 返回） |
| GET | `/api/conversations/{id}/events` | 对话 SSE 事件流 |
| POST | `/api/artifacts` | 上传 artifact |
| GET | `/api/artifacts/{id}/download` | 下载 artifact |
| POST | `/api/clients/generate` | 交叉编译客户端 |
| GET | `/api/clients/download` | 下载已编译客户端 |
| GET/POST | `/mcp` | MCP 端点 |

除 `/api/health` 和 WebSocket 外，均需 `Authorization: Bearer <api_token>`。

## REPL 命令

```
/sessions                   列出在线客户端
/use <install_id>           选择客户端
/exec <cmd>                 执行命令
/tool <name> <json>         调用远程工具
/agent <prompt>             与 AI Agent 对话
/back                       取消选择
/quit                       退出
```

## 仪表盘

```bash
make web-dev                  # 开发服务器 http://localhost:5173
make web-build                # 生产构建
```

## License

MIT
