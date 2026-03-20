# VectorShell Frontend Integration (React Pseudocode)

This pseudocode shows a minimal REPL-like frontend using:

1. REST for sending messages / tool calls
2. SSE for real-time agent/tool events

---

## 1) Shared API Client

```tsx
const API_BASE = process.env.NEXT_PUBLIC_API_BASE
const TOKEN = localStorage.getItem("vectorshell_token")

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      "Authorization": `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
      ...(init?.headers || {}),
    },
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`HTTP ${res.status}: ${text}`)
  }
  return (await res.json()) as T
}
```

---

## 2) Sessions Page

```tsx
type Session = {
  connection_id: string
  hostname: string
  username: string
  pid: number
  ip: string
  build_uuid: string
  last_heartbeat: number
}

function SessionsPage() {
  const [sessions, setSessions] = useState<Session[]>([])

  useEffect(() => {
    api<{sessions: Session[]}>("/api/sessions").then(r => setSessions(r.sessions))
  }, [])

  return (
    <div>
      {sessions.map(s => (
        <button key={s.connection_id} onClick={() => openConversation(s.connection_id)}>
          {s.hostname} ({s.username}) #{s.connection_id}
        </button>
      ))}
    </div>
  )
}
```

---

## 3) Create Conversation

```tsx
async function createConversation(connectionId: string) {
  return api<{conversation_id: string}>("/api/conversations", {
    method: "POST",
    body: JSON.stringify({ connection_id: connectionId, title: "web-repl" }),
  })
}
```

---

## 4) SSE Stream Hook

```tsx
type UiEvent =
  | { event: "conversation.started"; connection_id: string }
  | { event: "agent.message"; role: string; content: string; final?: boolean }
  | { event: "tool.started"; request_id: string; tool_name: string; args?: any }
  | { event: "tool.progress"; request_id: string; tool_name: string; percent: number }
  | { event: "tool.finished"; request_id: string; tool_name: string; ok: boolean; data?: any; error?: string }
  | { event: "conversation.finished"; ok: boolean }
  | { event: "error"; code: string; message: string }

function useConversationEvents(conversationId: string | null, onEvent: (e: UiEvent) => void) {
  useEffect(() => {
    if (!conversationId) return

    // EventSource cannot set Authorization header directly.
    // Recommended backend pattern: issue short-lived SSE ticket first.
    // Here we assume /api/conversations/{id}/events?token=... is supported.
    const es = new EventSource(
      `${API_BASE}/api/conversations/${conversationId}/events?token=${encodeURIComponent(TOKEN || "")}`
    )

    const handler = (ev: MessageEvent) => {
      try {
        onEvent(JSON.parse(ev.data))
      } catch (e) {
        console.error("Invalid SSE payload", e)
      }
    }

    es.onmessage = handler
    es.onerror = () => {
      // optionally reconnect/backoff
    }

    return () => es.close()
  }, [conversationId])
}
```

---

## 5) Send Message (Agent Chat)

```tsx
async function sendMessage(conversationId: string, message: string) {
  return api<{accepted: boolean; message_id: string}>(
    `/api/conversations/${conversationId}/messages`,
    {
      method: "POST",
      body: JSON.stringify({ message }),
    }
  )
}
```

---

## 6) File Upload Flow (artifact -> client)

```tsx
async function uploadArtifact(file: File) {
  const form = new FormData()
  form.append("file", file)

  const res = await fetch(`${API_BASE}/api/artifacts`, {
    method: "POST",
    headers: { "Authorization": `Bearer ${TOKEN}` },
    body: form,
  })
  if (!res.ok) throw new Error(await res.text())
  return res.json() as Promise<{artifact_id: string}>
}

async function putArtifactToClient(connectionId: string, artifactId: string, clientDst: string) {
  return api(`/api/sessions/${connectionId}/tools`, {
    method: "POST",
    body: JSON.stringify({
      tool_name: "upload_file",
      args: {
        src: { scope: "artifact", artifact_id: artifactId },
        dst: { scope: "client", path: clientDst },
      },
      timeout_ms: 120000,
    }),
  })
}
```

---

## 7) File Download Flow (client -> artifact -> browser)

```tsx
async function fetchClientFileAsArtifact(connectionId: string, clientSrc: string) {
  const result = await api<any>(`/api/sessions/${connectionId}/tools`, {
    method: "POST",
    body: JSON.stringify({
      tool_name: "download_file",
      args: {
        src: { scope: "client", path: clientSrc },
        dst: { scope: "artifact" },
      },
      timeout_ms: 120000,
    }),
  })
  return result?.data?.artifact_id as string
}

function downloadArtifact(artifactId: string) {
  window.location.href = `${API_BASE}/api/artifacts/${artifactId}/download?token=${encodeURIComponent(TOKEN || "")}`
}
```

---

## 8) REPL-like Component Sketch

```tsx
function ReplView({ conversationId }: { conversationId: string }) {
  const [input, setInput] = useState("")
  const [logs, setLogs] = useState<string[]>([])

  useConversationEvents(conversationId, (evt) => {
    switch (evt.event) {
      case "agent.message":
        setLogs(prev => [...prev, `Agent: ${evt.content}`])
        break
      case "tool.started":
        setLogs(prev => [...prev, `Tool start: ${evt.tool_name}`])
        break
      case "tool.progress":
        setLogs(prev => [...prev, `Tool progress: ${evt.tool_name} ${evt.percent}%`])
        break
      case "tool.finished":
        setLogs(prev => [...prev, `Tool done: ${evt.tool_name} ok=${evt.ok}`])
        break
      case "error":
        setLogs(prev => [...prev, `Error: ${evt.message}`])
        break
    }
  })

  const onSubmit = async () => {
    await sendMessage(conversationId, input)
    setLogs(prev => [...prev, `You: ${input}`])
    setInput("")
  }

  return (
    <div>
      <pre>{logs.join("\n")}</pre>
      <input value={input} onChange={e => setInput(e.target.value)} />
      <button onClick={onSubmit}>Send</button>
    </div>
  )
}
```

---

## 9) Notes

- For browser SSE with Bearer auth, prefer an SSE ticket endpoint or cookie-based session.
- Keep large file contents out of chat context; use artifact IDs and summaries.
- Surface tool events directly in UI so users can inspect what the agent actually did.
