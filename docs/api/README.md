# API Docs Index

This directory contains the draft API specification for exposing VectorShell Server to frontend clients and AI callers.

## Files

- `openapi.yaml`
  - Draft REST API spec (OpenAPI 3.0.3)
  - Covers sessions, tools, conversations, artifacts, and health endpoints

- `sse-event-schema.json`
  - JSON Schema for SSE event payloads from:
  - `GET /api/conversations/{conversation_id}/events`

- `frontend-react-pseudocode.md`
  - Minimal React integration flow (session select, conversation create, SSE subscribe, message send, artifact upload/download)

## Suggested Integration Order

1. Implement auth middleware (`Authorization: Bearer <token>`)
2. Implement `GET /api/health` and `GET /api/sessions`
3. Implement conversation create/send + SSE stream
4. Implement tool dispatch endpoint
5. Implement artifact upload/download endpoints
6. Frontend E2E: select session -> send message -> observe tool events -> download artifact

## Notes

- Use `PathRef` semantics (`scope: client/server/artifact`) to avoid path-side ambiguity.
- Keep large file content out of LLM context. Return summaries + artifact IDs.
- `download_file_chunk` is considered an internal transport detail; expose stable high-level tool APIs externally.
