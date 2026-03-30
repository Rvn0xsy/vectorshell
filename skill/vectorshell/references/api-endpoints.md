# VectorShell Server API Endpoints (Quick Reference)

This reference is for operating online clients through server API.

## Base variables

```bash
BASE_URL="https://127.0.0.1:8443"
API_TOKEN="<your-api-token>"
CONNECTION_ID="<connection-id>"
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

## 2) Conversation flow

Create conversation:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"connection_id\":\"${CONNECTION_ID}\",\"title\":\"api-session\"}" \
  "${BASE_URL}/api/conversations"
```

Send message:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"message":"collect basic host info"}' \
  "${BASE_URL}/api/conversations/${CONVERSATION_ID}/messages"
```

Subscribe SSE events:

```bash
curl -N "${BASE_URL}/api/conversations/${CONVERSATION_ID}/events?token=${API_TOKEN}"
```

## 3) Session tool calls

Exec:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"exec","args":{"command":"whoami"},"timeout_ms":120000}' \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

Read file:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"read_file","args":{"path":"/etc/hostname"}}' \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

Upload file to client (artifact -> client path):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"tool_name\":\"upload_file\",\"args\":{\"src\":{\"scope\":\"artifact\",\"artifact_id\":\"${ARTIFACT_ID}\"},\"dst\":{\"scope\":\"client\",\"path\":\"/tmp/upload.bin\"}}}" \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

Download file from client (client path -> artifact):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"download_file","args":{"src":{"scope":"client","path":"/etc/hostname"},"dst":{"scope":"artifact"}}}' \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

Windows PowerShell CLR tool:

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"tool_name":"powershell_clr","args":{"script":"Get-Process | Select-Object -First 3"},"timeout_ms":120000}' \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

Windows .NET assembly tool (artifact mode):

```bash
curl -s -H "Authorization: Bearer ${API_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"tool_name\":\"dotnet_assembly\",\"args\":{\"artifact_id\":\"${ARTIFACT_ID}\",\"runtime_version\":\"v4\",\"args\":[],\"patch_exit\":true},\"timeout_ms\":180000}" \
  "${BASE_URL}/api/sessions/${CONNECTION_ID}/tools"
```

## 4) Artifact operations

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

## 5) Common API errors

- `401 unauthorized`: token invalid/missing
- `404 not_found`: session/conversation/artifact missing
- `409 capability_mismatch`: selected client lacks tool capability
- `408 tool_timeout`: increase timeout or check client status
