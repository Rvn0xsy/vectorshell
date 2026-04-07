# VectorShell API Endpoints

## Base variables

```bash
BASE_URL="https://127.0.0.1:8443"
API_TOKEN="<your-api-token>"
INSTALL_ID="<install-id>"         # stable session ID (from /api/sessions)
CONVERSATION_ID="<conversation-id>"
ARTIFACT_ID="<artifact-id>"
```

## 1) Health and sessions

```bash
curl -s "${BASE_URL}/api/health"
```

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" "${BASE_URL}/api/sessions"
```

Response includes each session's `install_id`, `session_id`, `hostname`, `username`, `os`, `arch`, `build_uuid`.

## 2) Session events (SSE)

Subscribe to live exec/tool events for a session:

```bash
curl -N -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/events"
```

## 3) Conversation flow

Create conversation:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"install_id\":\"${INSTALL_ID}\",\"title\":\"ops-session\"}" \
  "${BASE_URL}/api/conversations"
```

Send message:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"message":"collect basic host info"}' \
  "${BASE_URL}/api/conversations/${CONVERSATION_ID}/messages"
```

Subscribe to conversation SSE events:

```bash
curl -N -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/conversations/${CONVERSATION_ID}/events"
```

## 4) Session tool calls

Exec:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"exec","args":{"command":"whoami"},"timeout_ms":120000}' \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Read file:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"read_file","args":{"path":"/etc/hostname"}}' \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Write file:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"write_file","args":{"path":"/tmp/test.txt","content":"hello world"}}' \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Upload file to client (artifact → client path):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"tool_name\":\"upload_file\",\"args\":{\"src\":{\"scope\":\"artifact\",\"artifact_id\":\"${ARTIFACT_ID}\"},\"dst\":{\"scope\":\"client\",\"path\":\"/tmp/upload.bin\"}}}" \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Download file from client (client path → artifact):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"download_file","args":{"src":{"scope":"client","path":"/etc/hostname"},"dst":{"scope":"artifact"}}}' \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Windows PowerShell CLR:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"powershell_clr","args":{"script":"Get-Process | Select-Object -First 3"},"timeout_ms":120000}' \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

Windows .NET assembly (artifact mode):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"tool_name\":\"dotnet_assembly\",\"args\":{\"artifact_id\":\"${ARTIFACT_ID}\",\"runtime_version\":\"v4\",\"args\":[],\"patch_exit\":true},\"timeout_ms\":180000}" \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/tools"
```

## 5) Artifact operations

Upload artifact:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -F "file=@./sample.bin" \
  "${BASE_URL}/api/artifacts"
```

Get artifact metadata:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/artifacts/${ARTIFACT_ID}"
```

Download artifact:

```bash
curl -L -H "Authorization: Bearer ${API_TOKEN}" \
  -o artifact.bin \
  "${BASE_URL}/api/artifacts/${ARTIFACT_ID}/download"
```

Delete artifact:

```bash
curl -s -X DELETE -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/artifacts/${ARTIFACT_ID}"
```

## 6) Session history

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/history"
```

Clean session history:

```bash
curl -s -X POST -H "Authorization: Bearer ${API_TOKEN}" \
  "${BASE_URL}/api/sessions/${INSTALL_ID}/clean"
```

## Common errors

| Code | Meaning |
|------|---------|
| `401 unauthorized` | Token invalid or missing |
| `404 not_found` | Session/conversation/artifact not found |
| `409 capability_mismatch` | Client lacks the requested tool capability |
| `408 tool_timeout` | Increase timeout or check client is online |
