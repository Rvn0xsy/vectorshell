---
name: vectorshell-server-api-operator
description: Operate online VectorShell clients strictly via VectorShell Server API. Use this skill whenever the user asks to list/select sessions, create conversations, send messages, subscribe to SSE events, dispatch remote tools, upload/download artifacts, or perform remote actions without direct host login.
---

# VectorShell Server API Operator

## Purpose

Use this skill to remotely operate connected clients **only through server API endpoints**.

This skill does not require direct SSH/RDP access and does not modify client binaries.

---

## When to use

Use this skill when tasks include:

- list online sessions and choose a target client
- create/send conversation messages through API
- watch SSE stream for tool/agent progress
- call remote tools on selected session
- upload artifacts for remote actions
- download artifacts/results from server API
- troubleshoot API-side failures (`unauthorized`, `not_found`, `capability_mismatch`, `tool_timeout`)

---

## Required inputs

1. `server_base_url` (example: `https://host:8443`)
2. `api_token`
3. task intent
4. optional target selector (`connection_id`, hostname, OS)

If target selector is missing and multiple sessions exist, ask one concise clarification question.

---

## API-first workflow

### 1) Health and session discovery

1. `GET /api/health`
2. `GET /api/sessions`
3. pick target `connection_id`

### 2) Conversation channel

1. `POST /api/conversations` with `connection_id`
2. `GET /api/conversations/{conversation_id}/events` (SSE)
3. `POST /api/conversations/{conversation_id}/messages`

### 3) Direct tool dispatch (when needed)

1. `POST /api/sessions/{connection_id}/tools`
2. body: `tool_name`, `args`, optional `timeout_ms`

### 4) Artifact workflow

1. `POST /api/artifacts` -> `artifact_id`
2. use `artifact_id` in tool args when supported
3. `GET /api/artifacts/{artifact_id}/download`

For concrete request/response examples, read:

- `references/api-endpoints.md`
- `references/sse-events.md`

---

## Tool selection policy

Preferred order:

1. `read_file` / `write_file`
2. `upload_file` / `download_file`
3. `exec`
4. Windows CLR tools (`powershell_clr`, `dotnet_assembly`) only if capability exists

For `.NET` payloads, prefer `artifact_id` over inline `content_base64`.

---

## Safety and logging rules

1. Never force unsupported tools.
2. Never expose full binary/base64 payloads in logs/UI.
3. Sanitize args shown to users:
   - `content_base64` -> `<base64:N chars>`
   - long strings -> truncated with original length marker
4. Prefer low-risk/reversible actions before destructive operations.

---

## Error handling playbook

### `401 unauthorized`

1. verify `Authorization: Bearer <api_token>`
2. verify token matches server `auth.api_token`

### `404 not_found`

1. verify `connection_id`/`conversation_id` exists
2. if conversation missing, recreate conversation and retry once

### `409 capability_mismatch`

1. verify selected session capabilities
2. switch tool or switch target session

### `408/timeout` (`tool_timeout`)

1. increase `timeout_ms`
2. verify client is online and heartbeats are fresh

### `conversation-event error`

1. read SSE `error.message`
2. check server logs for matching failure context

Use `references/api-endpoints.md` as the primary endpoint cookbook during execution.
Use `references/sse-events.md` to interpret SSE payloads and event fields.

---

## Output format

When reporting execution results, include:

1. selected target (`connection_id` or selection rule)
2. API calls performed (endpoint + intent)
3. tool calls performed (sanitized args)
4. outcomes/evidence (success/failure + key output)
5. next safe step
