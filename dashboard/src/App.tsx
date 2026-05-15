import { useEffect, useMemo, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import './App.css'

type Session = {
  install_id: string
  session_id: string
  hostname: string
  username: string
  pid: number
}

type ChatItem =
  | { kind: 'user'; text: string; ts: string }
  | { kind: 'assistant'; text: string; ts: string }
  | { kind: 'tool'; text: string; ts: string }
  | { kind: 'exec'; command: string; stdout?: string; stderr?: string; cwd?: string; env?: [string, string][]; exitCode?: number; durationMs?: number; ts: string }
  | { kind: 'system'; text: string; ts: string }

type StreamStatus = 'connected' | 'reconnecting' | 'disconnected'

const HELP_TEXT = [
  '/help - show help',
  '/exec <command> - execute remote command',
  '/upload - open upload dialog',
  '/download - open download dialog',
  '/clean - clear install_id history on server',
].join('\n')

/** Exponential backoff with jitter: 1s, 2s, 4s, ... cap at 30s */
function reconnectDelay(attempt: number): number {
  const base = Math.min(1000 * Math.pow(2, attempt), 30_000)
  const jitter = Math.random() * 500
  return Math.floor(base + jitter)
}

function now() {
  return new Date().toLocaleTimeString()
}

const MAX_SESSION_CACHE_SIZE = 10

/** Adds a new entry to sessionCache, evicting oldest if over limit. */
function cacheWithLimit<T extends Record<string, { conversationId: string; chat: ChatItem[] }>>(
  prev: T,
  key: string,
  value: { conversationId: string; chat: ChatItem[] }
): T {
  const next = { ...prev, [key]: value }
  const keys = Object.keys(next)
  if (keys.length > MAX_SESSION_CACHE_SIZE) {
    // Remove the oldest key (first one in insertion order)
    const oldest = keys[0]
    delete next[oldest]
  }
  return next
}

function sanitizeForDisplay(value: any): any {
  if (typeof value === 'string') {
    if (value.length > 240) {
      return `${value.slice(0, 240)}...[truncated:${value.length}]`
    }
    return value
  }
  if (Array.isArray(value)) {
    return value.map((v) => sanitizeForDisplay(v))
  }
  if (value && typeof value === 'object') {
    const out: Record<string, any> = {}
    for (const [k, v] of Object.entries(value)) {
      if (k === 'content_base64') {
        const len = typeof v === 'string' ? v.length : 0
        out[k] = `<base64:${len} chars>`
      } else {
        out[k] = sanitizeForDisplay(v)
      }
    }
    return out
  }
  return value
}

function App() {
  const defaultApiBase = (() => {
    const saved = localStorage.getItem('vectorshell:apiBase')
    if (saved) return saved
    if (typeof window !== 'undefined' && window.location.port !== '5173') {
      return window.location.origin
    }
    return 'http://127.0.0.1:8080'
  })()

  const [apiBase, setApiBase] = useState(
    () => defaultApiBase,
  )
  const [token, setToken] = useState(
    () => localStorage.getItem('vectorshell:token') || 'change-me-api-token',
  )
  const [showSettings, setShowSettings] = useState(false)
  const [sessions, setSessions] = useState<Session[]>([])
  const [selectedInstallId, setSelectedInstallId] = useState<string>('')
  const [conversationId, setConversationId] = useState<string>('')
  const [sessionCache, setSessionCache] = useState<Record<string, { conversationId: string; chat: ChatItem[] }>>({})
  const [sessionStreamStatus, setSessionStreamStatus] = useState<StreamStatus>('disconnected')

  const [chat, setChat] = useState<ChatItem[]>([])
  const [input, setInput] = useState('')
  const [logs, setLogs] = useState<string[]>([])

  const [showUpload, setShowUpload] = useState(false)
  const [showDownload, setShowDownload] = useState(false)
  const [showArtifactUpload, setShowArtifactUpload] = useState(false)
  const [uploadDst, setUploadDst] = useState('/tmp/upload.bin')
  const [downloadSrc, setDownloadSrc] = useState('/etc/hostname')
  const [downloadFilename, setDownloadFilename] = useState('download.bin')
  const [clientTarget, setClientTarget] = useState('linux-amd64')

  const [execHistory, setExecHistory] = useState<string[]>([])
  const execHistPos = useRef<number>(-1)

  const [logFilter, setLogFilter] = useState<'all' | 'tool' | 'agent' | 'exec'>('all')
  const [activeExecMeta, setActiveExecMeta] = useState<Record<number, boolean>>({})

  const chatEndRef = useRef<HTMLDivElement | null>(null)
  const chatBodyRef = useRef<HTMLDivElement | null>(null)
  const sessionEsRef = useRef<EventSource | null>(null)
  const sessionReconnectTimerRef = useRef<number | null>(null)
  const selectedInstallIdRef = useRef<string>('')
  const conversationIdRef = useRef<string>('')
  const sessionReconnectAttemptRef = useRef<number>(0)

  const api = async (path: string, init: RequestInit = {}) => {
    const res = await fetch(`${apiBase}${path}`, {
      ...init,
      headers: {
        Authorization: `Bearer ${token}`,
        ...(init.headers || {}),
      },
    })
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}: ${await res.text()}`)
    }
    const ct = res.headers.get('content-type') || ''
    if (ct.includes('application/json')) return res.json()
    return res.text()
  }

  const pushLog = (line: string) => {
    setLogs((x) => [...x, `[${now()}] ${line}`])
  }

  const filteredLogs = useMemo(() => {
    if (logFilter === 'all') return logs
    return logs.filter((l) => l.toLowerCase().includes(logFilter))
  }, [logs, logFilter])

  const connectSessionSse = (installId: string) => {
    setSessionStreamStatus('reconnecting')
    if (sessionEsRef.current) sessionEsRef.current.close()
    const es = new EventSource(
      `${apiBase}/api/sessions/${installId}/events?token=${encodeURIComponent(token)}`,
    )
    es.onmessage = (ev) => {
      setSessionStreamStatus('connected')
      sessionReconnectAttemptRef.current = 0
      let data: any = ev.data
      try {
        data = JSON.parse(ev.data)
      } catch {
        // ignore
      }
      pushLog(`session-event ${typeof data === 'string' ? data : data.event || 'unknown'}`)
      // Only process events for the active conversation (if any)
      const evConvId = data.conversation_id || ''
      if (conversationIdRef.current && evConvId && evConvId !== conversationIdRef.current) {
        return // skip events for other conversations
      }
      if (typeof data === 'object' && data.event === 'exec.result') {
        setChat((prev) => [
          ...prev,
          {
            kind: 'exec',
            ts: now(),
            command: data.command || '',
            stdout: data.stdout,
            stderr: data.stderr,
            cwd: data.cwd,
            env: data.env,
            exitCode: data.exit_code,
            durationMs: data.duration_ms,
          },
        ])
      }
      if (typeof data === 'object' && data.event === 'tool.result') {
        setChat((prev) => [
          ...prev,
          { kind: 'tool', ts: now(), text: `Tool Use: ${data.tool_name} -> ok=${data.ok}` },
        ])
      }
      if (typeof data === 'object') {
        if (data.event === 'error') {
          const detail = String(data.message || 'unknown error')
          setChat((prev) => [...prev, { kind: 'system', ts: now(), text: `Conversation error: ${detail}` }])
        }
        if (data.event === 'agent.message' && data.content) {
          setChat((prev) => [...prev, { kind: 'assistant', ts: now(), text: data.content }])
        }
        if (data.event === 'tool.started') {
          const safeArgs = sanitizeForDisplay(data.args || {})
          setChat((prev) => [
            ...prev,
            { kind: 'tool', ts: now(), text: `Tool Use: ${data.tool_name} ${JSON.stringify(safeArgs)}` },
          ])
        }
        if (data.event === 'tool.finished') {
          setChat((prev) => [
            ...prev,
            { kind: 'tool', ts: now(), text: `Tool Result: ${data.tool_name} ok=${data.ok} duration=${data.duration_ms}ms` },
          ])
        }
      }
    }
    es.onerror = () => {
      setSessionStreamStatus('reconnecting')
      pushLog('session-event stream disconnected')
      if (sessionReconnectTimerRef.current) {
        window.clearTimeout(sessionReconnectTimerRef.current)
      }
      const attempt = sessionReconnectAttemptRef.current
      const delay = reconnectDelay(attempt)
      sessionReconnectAttemptRef.current = attempt + 1
      pushLog(`session reconnect in ${delay}ms (attempt ${attempt + 1})`)
      sessionReconnectTimerRef.current = window.setTimeout(() => {
        if (selectedInstallIdRef.current === installId) {
          connectSessionSse(installId)
        }
      }, delay)
    }
    sessionEsRef.current = es
  }

  const loadSessions = async () => {
    const data = await api('/api/sessions')
    setSessions(data.sessions || [])
    pushLog(`loaded sessions: ${(data.sessions || []).length}`)
  }

  const createConversationForSession = async (installId: string) => {
    const conv = await api('/api/conversations', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ install_id: installId, title: 'webapp' }),
    })
    const convId = String(conv.conversation_id || '')
    if (!convId) throw new Error('server returned empty conversation id')
    setConversationId(convId)
    pushLog(`conversation ready: ${convId}`)
    return convId
  }

  const onSelectSession = async (installId: string) => {
    if (selectedInstallId && selectedInstallId !== installId) {
      setSessionCache((prev) => cacheWithLimit(prev, selectedInstallId, { conversationId, chat }))
    }

    setSelectedInstallId(installId)
    localStorage.setItem('vectorshell:selectedInstallId', installId)

    const cached = sessionCache[installId]
    if (cached) {
      setConversationId(cached.conversationId)
      setChat(cached.chat)
      connectSessionSse(installId)
      pushLog(`restored cached session context: ${installId} conv=${cached.conversationId}`)
      return
    }

    setChat([{ kind: 'system', ts: now(), text: `selected session ${installId}` }])
    connectSessionSse(installId)
    const history = await api(`/api/sessions/${installId}/history`)
    const restoredConversationId = String(history.conversation_id || '')
    if (restoredConversationId) {
      setConversationId(restoredConversationId)
    }
    const restoredChat: ChatItem[] = (history.messages || []).map((m: any) => {
      if (m.role === 'assistant') return { kind: 'assistant', ts: now(), text: m.content }
      if (m.role === 'user') return { kind: 'user', ts: now(), text: m.content }
      return { kind: 'system', ts: now(), text: `${m.role}: ${m.content}` }
    })
    setChat((prev) => {
      const seed = [{ kind: 'system', ts: now(), text: `selected session ${installId}` } as ChatItem]
      return [...seed, ...restoredChat, ...prev.filter((x) => x.kind === 'system' && x.text.includes('selected session'))]
    })

    if (!restoredConversationId) {
      await createConversationForSession(installId)
    }
  }

  const sendChatMessage = async (text: string) => {
    if (!selectedInstallId) {
      throw new Error('session not selected')
    }
    let activeConversationId = conversationId
    if (!activeConversationId) {
      activeConversationId = await createConversationForSession(selectedInstallId)
    }
    await api(`/api/conversations/${activeConversationId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message: text }),
    })
  }

  const callTool = async (tool_name: string, args: any) => {
    if (!selectedInstallId) throw new Error('session not selected')
    return api(`/api/sessions/${selectedInstallId}/tools`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ tool_name, args, timeout_ms: 180000 }),
    })
  }

  const onSubmitInput = async () => {
    const text = input.trim()
    if (!text) return
    setInput('')

    if (text === '/help') {
      setChat((prev) => [...prev, { kind: 'system', ts: now(), text: HELP_TEXT }])
      return
    }

    if (text.startsWith('/exec ')) {
      const command = text.slice(6).trim()
      if (!command) return
      setExecHistory((x) => [...x, command])
      execHistPos.current = execHistory.length + 1
      setChat((prev) => [...prev, { kind: 'user', ts: now(), text }])
      const result = await callTool('exec', { command })
      const d = result.data || {}
      setChat((prev) => [
        ...prev,
        {
          kind: 'exec',
          ts: now(),
          command,
          stdout: d.stdout,
          stderr: d.stderr,
          cwd: d.cwd,
          env: d.env,
          exitCode: d.exit_code,
          durationMs: result.duration_ms,
        },
      ])
      return
    }

    if (text === '/upload') {
      setShowUpload(true)
      return
    }

    if (text === '/download') {
      setShowDownload(true)
      return
    }

    if (text === '/clean') {
      if (!selectedInstallId) throw new Error('session not selected')
      await api(`/api/sessions/${selectedInstallId}/clean`, { method: 'POST' })
      setChat([{ kind: 'system', ts: now(), text: 'History cleaned for current install_id.' }])
      pushLog('history cleaned via /clean')
      return
    }

    setChat((prev) => [...prev, { kind: 'user', ts: now(), text }])
    await sendChatMessage(text)
  }

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      onSubmitInput().catch((err) =>
        setChat((p) => [...p, { kind: 'system', ts: now(), text: String(err) }]),
      )
      return
    }

    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return
    if (!execHistory.length) return
    e.preventDefault()

    if (e.key === 'ArrowUp') {
      execHistPos.current = Math.max(0, execHistPos.current - 1)
      setInput(`/exec ${execHistory[execHistPos.current] || ''}`)
    } else {
      execHistPos.current = Math.min(execHistory.length, execHistPos.current + 1)
      const cmd = execHistory[execHistPos.current] || ''
      setInput(cmd ? `/exec ${cmd}` : '')
    }
  }

  useEffect(() => {
    if (!selectedInstallId) return
    setSessionCache((prev) => cacheWithLimit(prev, selectedInstallId, { conversationId, chat }))
  }, [selectedInstallId, conversationId, chat])

  useEffect(() => {
    selectedInstallIdRef.current = selectedInstallId
  }, [selectedInstallId])

  useEffect(() => {
    conversationIdRef.current = conversationId
  }, [conversationId])

  const submitUpload = async (file: File | null) => {
    if (!file) throw new Error('no file selected')
    if (!uploadDst.trim()) throw new Error('destination path required')
    const form = new FormData()
    form.append('file', file)
    const uploadRes = await fetch(`${apiBase}/api/artifacts`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: form,
    })
    if (!uploadRes.ok) throw new Error(await uploadRes.text())
    const artifact = await uploadRes.json()
    const result = await callTool('upload_file', {
      src: { scope: 'artifact', artifact_id: artifact.artifact_id },
      dst: { scope: 'client', path: uploadDst.trim() },
    })
    setChat((prev) => [
      ...prev,
      { kind: 'tool', ts: now(), text: `Tool Use: upload_file src=artifact:${artifact.artifact_id} dst=${uploadDst}` },
      { kind: 'tool', ts: now(), text: `Tool Result: upload_file ok=${result.ok}` },
    ])
    setShowUpload(false)
  }

  const submitArtifactUpload = async (file: File | null) => {
    if (!file) throw new Error('no file selected')
    const form = new FormData()
    form.append('file', file)
    const uploadRes = await fetch(`${apiBase}/api/artifacts`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: form,
    })
    if (!uploadRes.ok) throw new Error(await uploadRes.text())
    const artifact = await uploadRes.json()
    pushLog(`artifact uploaded: ${artifact.artifact_id} (${artifact.size_bytes || 0} bytes)`)
    setChat((prev) => [
      ...prev,
      { kind: 'tool', ts: now(), text: `Artifact uploaded: ${artifact.artifact_id} size=${artifact.size_bytes || 0}` },
    ])
    setShowArtifactUpload(false)
  }

  const submitDownload = async () => {
    if (!downloadSrc.trim()) throw new Error('source path required')
    const result = await callTool('download_file', {
      src: { scope: 'client', path: downloadSrc.trim() },
      dst: { scope: 'artifact' },
    })
    const artifactId = result?.data?.artifact_id
    if (!artifactId) throw new Error('download result missing artifact_id')
    const res = await fetch(`${apiBase}/api/artifacts/${artifactId}/download?token=${encodeURIComponent(token)}`)
    if (!res.ok) throw new Error(await res.text())
    const blob = await res.blob()
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = downloadFilename || 'download.bin'
    document.body.appendChild(a)
    a.click()
    a.remove()
    URL.revokeObjectURL(url)

    setChat((prev) => [
      ...prev,
      { kind: 'tool', ts: now(), text: `Tool Use: download_file src=${downloadSrc} dst=artifact` },
      { kind: 'tool', ts: now(), text: `Tool Result: download_file artifact_id=${artifactId}` },
    ])
    setShowDownload(false)
  }

  const generateAndDownloadClient = async () => {
    const generated = await api('/api/clients/generate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ target: clientTarget }),
    })
    pushLog(`client generated: target=${generated.target} file=${generated.file}`)

    const res = await fetch(
      `${apiBase}/api/clients/download?target=${encodeURIComponent(clientTarget)}&token=${encodeURIComponent(token)}`,
    )
    if (!res.ok) throw new Error(await res.text())
    const blob = await res.blob()
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = generated.file || (clientTarget.includes('windows') ? 'vectorshell-client.exe' : 'vectorshell-client')
    document.body.appendChild(a)
    a.click()
    a.remove()
    URL.revokeObjectURL(url)
    setChat((prev) => [...prev, { kind: 'system', ts: now(), text: `Client generated and downloaded (${clientTarget})` }])
  }

  useEffect(() => {
    if (chatBodyRef.current) {
      chatBodyRef.current.scrollTop = chatBodyRef.current.scrollHeight
    }
    chatEndRef.current?.scrollIntoView({ behavior: 'auto' })
  }, [chat])

  useEffect(() => {
    loadSessions()
      .then(() => {
        const remembered = localStorage.getItem('vectorshell:selectedInstallId')
        if (remembered && !selectedInstallId) {
          return onSelectSession(remembered)
        }
      })
      .catch((e) => pushLog(`load sessions error: ${String(e)}`))
    return () => {
      sessionEsRef.current?.close()
      setSessionStreamStatus('disconnected')
      if (sessionReconnectTimerRef.current) {
        window.clearTimeout(sessionReconnectTimerRef.current)
      }
    }
  }, [])

  useEffect(() => {
    if (!sessions.length) return
    const remembered = localStorage.getItem('vectorshell:selectedInstallId')
    if (!remembered) return
    if (sessions.some((s) => s.install_id === remembered) && selectedInstallId !== remembered) {
      onSelectSession(remembered).catch((e) => pushLog(`restore session failed: ${String(e)}`))
    }
  }, [sessions])

  useEffect(() => {
    localStorage.setItem('vectorshell:apiBase', apiBase)
  }, [apiBase])

  useEffect(() => {
    localStorage.setItem('vectorshell:token', token)
  }, [token])

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-brand">
          <div className="topbar-logo">
            <svg viewBox="0 0 28 28" fill="none" xmlns="http://www.w3.org/2000/svg">
              <polygon points="14,2 26,8 26,20 14,26 2,20 2,8" stroke="#00e5cc" strokeWidth="1.5" fill="none"/>
              <polygon points="14,6 22,10 22,18 14,22 6,18 6,10" stroke="#00e5cc" strokeWidth="1" fill="rgba(0,229,204,0.08)"/>
              <circle cx="14" cy="14" r="3" fill="#00e5cc"/>
              <line x1="14" y1="2" x2="14" y2="6" stroke="#00e5cc" strokeWidth="1"/>
              <line x1="14" y1="22" x2="14" y2="26" stroke="#00e5cc" strokeWidth="1"/>
              <line x1="26" y1="8" x2="22" y2="10" stroke="#00e5cc" strokeWidth="1"/>
              <line x1="6" y1="18" x2="2" y2="20" stroke="#00e5cc" strokeWidth="1"/>
              <line x1="2" y1="8" x2="6" y2="10" stroke="#00e5cc" strokeWidth="1"/>
              <line x1="22" y1="18" x2="26" y2="20" stroke="#00e5cc" strokeWidth="1"/>
            </svg>
          </div>
          <h1>VectorShell</h1>
        </div>
        <div className="topbar-status">
          <div className="topbar-status-dot" />
          <span>ONLINE</span>
        </div>
        <button className="gear" onClick={() => setShowSettings(true)} aria-label="Settings">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="2.5" stroke="currentColor" strokeWidth="1.5"/>
            <path d="M8 1v2M8 13v2M1 8h2M13 8h2M3.05 3.05l1.41 1.41M11.54 11.54l1.41 1.41M3.05 12.95l1.41-1.41M11.54 4.46l1.41-1.41" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
          </svg>
        </button>
      </header>

      <div className="layout">
        <aside className="session-panel">
          <div className="panel-section">
            <div className="panel-title">Sessions</div>
            <div className="session-list">
              {sessions.map((s) => (
                <button
                  key={s.install_id}
                  className={`session-item ${selectedInstallId === s.install_id ? 'active' : ''}`}
                  onClick={() => onSelectSession(s.install_id)}
                >
                  <div className="session-host">{s.hostname}</div>
                  <div className="session-meta">{s.username}<span> · </span>{s.install_id.slice(0, 8)}</div>
                </button>
              ))}
            </div>
            <button className="refresh-btn" onClick={() => loadSessions().catch((e) => pushLog(String(e)))}>↻ Refresh</button>
          </div>

          <div className="client-card">
            <div className="panel-title">Generate Client</div>
            <select value={clientTarget} onChange={(e) => setClientTarget(e.target.value)}>
              <option value="linux-amd64">linux-amd64</option>
              <option value="linux-arm64">linux-arm64</option>
              <option value="windows-amd64">windows-amd64</option>
              <option value="windows-arm64">windows-arm64</option>
              <option value="macos-amd64">macos-amd64</option>
              <option value="macos-arm64">macos-arm64</option>
            </select>
            <button onClick={() => generateAndDownloadClient().catch((e) => pushLog(`generate client failed: ${String(e)}`))}>
              Generate + Download
            </button>
          </div>

          <div className="logs-section">
            <div className="logs-toolbar">
              <select value={logFilter} onChange={(e) => setLogFilter(e.target.value as any)}>
                <option value="all">all</option>
                <option value="agent">agent</option>
                <option value="tool">tool</option>
                <option value="exec">exec</option>
              </select>
              <button onClick={() => setLogs([])}>Clear</button>
            </div>
            <pre className="logs-box">{filteredLogs.join('\n')}</pre>
          </div>
        </aside>

        <section className="chat-panel">
          <div className="chat-header">
            <div className="chat-header-top">conversation: <span>{conversationId || '-'}</span></div>
            <div className="stream-status-row">
              <span className={`stream-pill ${sessionStreamStatus}`}>stream: {sessionStreamStatus}</span>
            </div>
          </div>
          <div className="chat-body" ref={chatBodyRef}>
            {chat.map((m, idx) => {
              if (m.kind === 'tool') {
                const lower = m.text.toLowerCase()
                const cls = lower.includes('result:') && lower.includes('ok=true') ? 'tool-row tool-ok'
                  : lower.includes('result:') && (lower.includes('ok=false') || lower.includes('error')) ? 'tool-row tool-err'
                  : lower.includes('tool use:') ? 'tool-row tool-start'
                  : 'tool-row'
                return <div key={idx} className={cls}>{m.text}</div>
              }
              if (m.kind === 'system') {
                return <div key={idx} className="system-row">{m.text}</div>
              }
              if (m.kind === 'assistant') {
                return (
                  <div key={idx} className="bubble left">
                    <div className="meta">Agent · {m.ts}</div>
                    <div className="md"><ReactMarkdown remarkPlugins={[remarkGfm]}>{m.text}</ReactMarkdown></div>
                  </div>
                )
              }
              if (m.kind === 'exec') {
                const open = activeExecMeta[idx] || false
                return (
                  <div key={idx} className="bubble left exec-card">
                    <div className="meta">exec · {m.ts} · exit={m.exitCode} · {m.durationMs}ms</div>
                    <div className="exec-command">{m.command}</div>
                    {m.stdout ? <pre className="stdout">{m.stdout}</pre> : null}
                    {m.stderr ? <pre className="stderr">{m.stderr}</pre> : null}
                    <button
                      className="fold"
                      onClick={() => setActiveExecMeta((s) => ({ ...s, [idx]: !open }))}
                    >
                      {open ? 'Hide meta' : 'Show meta (cwd/env)'}
                    </button>
                    {open ? (
                      <div className="meta-box">
                        <div><strong>cwd:</strong> {m.cwd || '-'}</div>
                        <details>
                          <summary>env</summary>
                          <pre>{(m.env || []).map(([k, v]) => `${k}=${v}`).join('\n')}</pre>
                        </details>
                      </div>
                    ) : null}
                  </div>
                )
              }
              return (
                <div key={idx} className="bubble right">
                  <div className="meta">You · {m.ts}</div>
                  <div>{m.text}</div>
                </div>
              )
            })}
            <div ref={chatEndRef} />
          </div>

          <div className="chat-input-strip">
            <div className="chat-tools">
              <button title="Upload file to client" aria-label="Upload file to client" onClick={() => setShowUpload(true)}>
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                  <path d="M7 9V1M4 6l3-3 3 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                  <path d="M1 10v2a1 1 0 001 1h10a1 1 0 001-1v-2" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
                </svg>
              </button>
              <button title="Download file from client" aria-label="Download file from client" onClick={() => setShowDownload(true)}>
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                  <path d="M7 1v8M4 5l3 3 3-3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                  <path d="M1 4v2a1 1 0 001 1h10a1 1 0 001-1V4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
                </svg>
              </button>
              <button title="Upload artifact" aria-label="Upload artifact" onClick={() => setShowArtifactUpload(true)}>
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                  <rect x="1" y="1" width="12" height="12" rx="2" stroke="currentColor" strokeWidth="1.5"/>
                  <path d="M4 7l2-2 2 2 2-2" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                  <path d="M7 5v4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
                </svg>
              </button>
            </div>
            <div className="chat-input-row">
              <input
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={onInputKeyDown}
                placeholder="Type message or /help /exec"
              />
              <button className="send-btn" onClick={() => onSubmitInput().catch((e) => setChat((p) => [...p, { kind: 'system', ts: now(), text: String(e) }]))}>
                Send
              </button>
            </div>
          </div>
        </section>
      </div>

      {showSettings ? (
        <div className="modal-mask" onClick={() => setShowSettings(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>Connection Settings</h3>
            <label>
              Server Base (API + WebSocket same port)
              <input value={apiBase} onChange={(e) => setApiBase(e.target.value)} />
            </label>
            <label>Token<input value={token} onChange={(e) => setToken(e.target.value)} /></label>
            <div className="row-end"><button onClick={() => setShowSettings(false)}>Close</button></div>
          </div>
        </div>
      ) : null}

      {showUpload ? (
        <UploadDialog
          dst={uploadDst}
          onDstChange={setUploadDst}
          onClose={() => setShowUpload(false)}
          onSubmit={submitUpload}
        />
      ) : null}

      {showDownload ? (
        <DownloadDialog
          src={downloadSrc}
          filename={downloadFilename}
          onSrcChange={setDownloadSrc}
          onFilenameChange={setDownloadFilename}
          onClose={() => setShowDownload(false)}
          onSubmit={submitDownload}
        />
      ) : null}

      {showArtifactUpload ? (
        <ArtifactUploadDialog
          onClose={() => setShowArtifactUpload(false)}
          onSubmit={submitArtifactUpload}
        />
      ) : null}
    </div>
  )
}

function UploadDialog({
  dst,
  onDstChange,
  onClose,
  onSubmit,
}: {
  dst: string
  onDstChange: (v: string) => void
  onClose: () => void
  onSubmit: (file: File | null) => Promise<void>
}) {
  const fileRef = useRef<HTMLInputElement | null>(null)
  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Upload File</h3>
        <label>Local file<input ref={fileRef} type="file" /></label>
        <label>Client destination<input value={dst} onChange={(e) => onDstChange(e.target.value)} /></label>
        <div className="row-end">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => onSubmit(fileRef.current?.files?.[0] || null).catch(console.error)}>Upload</button>
        </div>
      </div>
    </div>
  )
}

function DownloadDialog({
  src,
  filename,
  onSrcChange,
  onFilenameChange,
  onClose,
  onSubmit,
}: {
  src: string
  filename: string
  onSrcChange: (v: string) => void
  onFilenameChange: (v: string) => void
  onClose: () => void
  onSubmit: () => Promise<void>
}) {
  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Download File</h3>
        <label>Client source<input value={src} onChange={(e) => onSrcChange(e.target.value)} /></label>
        <label>Local filename<input value={filename} onChange={(e) => onFilenameChange(e.target.value)} /></label>
        <div className="row-end">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => onSubmit().catch(console.error)}>Download</button>
        </div>
      </div>
    </div>
  )
}

function ArtifactUploadDialog({
  onClose,
  onSubmit,
}: {
  onClose: () => void
  onSubmit: (file: File | null) => Promise<void>
}) {
  const fileRef = useRef<HTMLInputElement | null>(null)
  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Upload Artifact</h3>
        <label>Local file<input ref={fileRef} type="file" /></label>
        <div className="row-end">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => onSubmit(fileRef.current?.files?.[0] || null).catch(console.error)}>Upload</button>
        </div>
      </div>
    </div>
  )
}

export default App
