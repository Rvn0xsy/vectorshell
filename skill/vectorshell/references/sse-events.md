# VectorShell SSE Events Reference

This document summarizes SSE payloads used during remote operations via server API.

## Streams

1. Conversation stream:

- `GET /api/conversations/{conversation_id}/events`

2. Session stream:

- `GET /api/sessions/{connection_id}/events`

## Base fields

Most events contain:

- `event` (string)
- `conversation_id` (string; may be empty for session-level tool events)
- `timestamp` (RFC3339)

---

## Conversation stream events

### 1) `conversation.started`

Meaning: conversation execution started.

Typical fields:

- `conversation_id`
- `connection_id`
- `timestamp`

### 2) `agent.message`

Meaning: assistant output chunk/final message.

Typical fields:

- `role` (`assistant`)
- `content`
- `final` (bool)

### 3) `tool.started`

Meaning: a remote tool call started.

Typical fields:

- `request_id` (optional depending on emitter path)
- `tool_name`
- `args` (sanitized)

### 4) `tool.progress`

Meaning: tool in progress (commonly upload/download).

Typical fields:

- `tool_name`
- `percent` or `detail`

### 5) `tool.finished`

Meaning: tool completed.

Typical fields:

- `tool_name`
- `ok`
- `duration_ms`
- optional `data`
- optional `error`

### 6) `conversation.finished`

Meaning: conversation execution completed.

Typical fields:

- `ok`

### 7) `error`

Meaning: conversation-level failure.

Typical fields:

- `code`
- `message`

Use `message` as primary user-facing error detail.

---

## Session stream events

Session stream is mostly for direct command/tool feedback bound to a connection.

### 1) `exec.result`

Typical fields:

- `command`
- `exit_code`
- `duration_ms`
- `stdout`
- `stderr`

### 2) `tool.result`

Typical fields:

- `id`
- `tool_name`
- `ok`
- `duration_ms`
- `data`
- `error`

---

## Consumer guidance

1. Always handle unknown events gracefully.
2. For `error` events, surface `message` directly.
3. Do not render full `content_base64` if present in args/data.
4. Treat event ordering as best-effort; reconnect logic may miss transient events.

---

## Canonical schema reference

For conversation stream JSON schema, see:

- `api-docs/sse-event-schema.json`
