# VectorShell SSE Events Reference

## Streams

1. **Session stream** — live exec/tool feedback:
   ```
   GET /api/sessions/{install_id}/events
   ```
   Authorization: `Authorization: Bearer <token>`

2. **Conversation stream** — agent/tool lifecycle:
   ```
   GET /api/conversations/{conversation_id}/events
   ```
   Authorization: `Authorization: Bearer <token>`

## Common fields

Most events include:

- `event` (string) — event type name
- `timestamp` (RFC3339)

---

## Session stream events

### `exec.result`

Fired when a raw `/exec` command completes.

Typical fields:
- `command`
- `exit_code`
- `duration_ms`
- `stdout`
- `stderr`
- `cwd`
- `env` (array of `["KEY=value", ...]`)

### `tool.result`

Fired when a tool call completes.

Typical fields:
- `id`
- `tool_name`
- `ok` (bool)
- `duration_ms`
- `data` (tool-specific result, optional)
- `error` (string, optional)

---

## Conversation stream events

### `conversation.started`

Conversation execution started.

Typical fields:
- `conversation_id`
- `install_id`
- `timestamp`

### `agent.message`

Assistant output chunk or final message.

Typical fields:
- `role` (`assistant`)
- `content`
- `final` (bool)

### `tool.started`

A remote tool call has started.

Typical fields:
- `tool_name`
- `args` (sanitized — `content_base64` truncated, long strings noted)

### `tool.progress`

Tool in progress (upload/download).

Typical fields:
- `tool_name`
- `percent` or `detail`

### `tool.finished`

Tool completed.

Typical fields:
- `tool_name`
- `ok` (bool)
- `duration_ms`
- `data` (optional)
- `error` (optional)

### `conversation.finished`

Conversation execution completed.

Typical fields:
- `ok`

### `error`

Conversation-level failure.

Typical fields:
- `code`
- `message`

Use `message` as the primary user-facing error detail.

---

## Consumer guidance

1. Handle unknown events gracefully (ignore or log).
2. For `error` events, surface `message` directly to the user.
3. Do not render full `content_base64` in logs or UI — truncate to `<base64:N chars>`.
4. Event ordering is best-effort; reconnect logic may miss transient events.
5. Reconnection strategy: re-subscribe and replay from last known state.

For conversation SSE schema, see: `docs/api/sse-event-schema.json`.
