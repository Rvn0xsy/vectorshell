# VectorShell

VectorShell 是一个使用 Rust 编写的 AI 驱动远程命令执行平台。

- **Server（服务端）**：承载 AI Agent、管理客户端并下发命令。
- **Client（客户端）**：通过 WebSocket 回连服务端，执行命令并返回结果。
- **Shared（共享库）**：定义服务端与客户端共用的协议类型。

## 快速开始

构建全部模块：

```bash
cargo build
```

启动服务端：

```bash
./target/debug/vectorshell-server --config config/config.toml
```

启动本地客户端：

```bash
./target/debug/vectorshell-client
```

## 生成客户端二进制

使用 `config/config.toml` 中的配置生成带嵌入参数的客户端：

```bash
./target/debug/vectorshell-server --config config/config.toml generate-client --target linux-amd64
```

常用目标平台：

- `linux-amd64`
- `linux-arm64`
- `windows-amd64`
- `windows-arm64`
- `macos-amd64`
- `macos-arm64`

输出目录：

```text
build/clients/
```

## 客户端嵌入配置（编译期）

生成的客户端会在编译期嵌入以下参数：

- `VECTOR_SERVER_URL`
- `VECTOR_AUTH_TOKEN`
- `VECTOR_RECONNECT_INTERVAL`
- `VECTOR_INSECURE_TLS`

因此修改 `config.toml` 后，需要重新执行 `generate-client` 才会生效。

## TLS / WSS 说明

- 当 `[tls].enabled = true` 且证书/私钥配置正确时，服务端支持 `wss://`。
- 客户端支持 `wss://`。
- 自签名证书场景下，请在生成客户端前将 `client.insecure_tls = true`。

## Windows 系统代理说明

Windows 版客户端支持系统代理自动发现，顺序如下：

1. WinHTTP 自动代理（PAC/WPAD）
2. 系统手动代理（Internet Settings / `ProxyServer`）

若发现可用代理，客户端会先建立 HTTP CONNECT 隧道，再进行 WS/WSS 握手。

## 服务端 REPL 命令

- `clients`
- `use <client_id>`
- `exec <command>`
- `agent <task>`
- `agent-exec <task>`
