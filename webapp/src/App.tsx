import { useEffect, useMemo, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import './App.css'

type Session = {
  connection_id: string
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

const HELP_TEXT = [
  '/help - show help',
  '/exec <command> - execute remote command',
  '/upload - open upload dialog',
  '/download - open download dialog',
  '/clean - clear install_id history on server',
].join('\n')

function now() {
  return new Date().toLocaleTimeString()
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
    () => localStorage.getItem('vectorshell:token') || 'vectorshell-secret',
  )
  const [showSettings, setShowSettings] = useState(false)

  const [sessions, setSessions] = useState<Session[]>([])
  const [selectedSession, setSelectedSession] = useState<string>('')
  const [conversationId, setConversationId] = useState<string>('')
  const [sessionCache, setSessionCache] = useState<Record<string, { conversationId: string; chat: ChatItem[] }>>({})

  const [chat, setChat] = useState<ChatItem[]>([])
  const [input, setInput] = useState('')
  const [logs, setLogs] = useState<string[]>([])

  const [showUpload, setShowUpload] = useState(false)
  const [showDownload, setShowDownload] = useState(false)
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
  const convEsRef = useRef<EventSource | null>(null)

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

  const connectSessionSse = (connectionId: string) => {
    if (sessionEsRef.current) sessionEsRef.current.close()
    const es = new EventSource(
      `${apiBase}/api/sessions/${connectionId}/events?token=${encodeURIComponent(token)}`,
    )
    es.onmessage = (ev) => {
      let data: any = ev.data
      try {
        data = JSON.parse(ev.data)
      } catch {
        // ignore
      }
      pushLog(`session-event ${typeof data === 'string' ? data : data.event || 'unknown'}`)
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
    }
    es.onerror = () => pushLog('session-event stream disconnected')
    sessionEsRef.current = es
  }

  const connectConversationSse = (convId: string) => {
    if (convEsRef.current) convEsRef.current.close()
    const es = new EventSource(
      `${apiBase}/api/conversations/${convId}/events?token=${encodeURIComponent(token)}`,
    )
    es.onmessage = (ev) => {
      let data: any = ev.data
      try {
        data = JSON.parse(ev.data)
      } catch {
        // ignore
      }
      if (typeof data === 'object') {
        pushLog(`conversation-event ${data.event || 'unknown'}`)
        if (data.event === 'agent.message' && data.content) {
          setChat((prev) => [...prev, { kind: 'assistant', ts: now(), text: data.content }])
        }
        if (data.event === 'tool.started') {
          setChat((prev) => [
            ...prev,
            { kind: 'tool', ts: now(), text: `Tool Use: ${data.tool_name} ${JSON.stringify(data.args || {})}` },
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
    es.onerror = () => pushLog('conversation-event stream disconnected')
    convEsRef.current = es
  }

  const loadSessions = async () => {
    const data = await api('/api/sessions')
    setSessions(data.sessions || [])
    pushLog(`loaded sessions: ${(data.sessions || []).length}`)
  }

  const onSelectSession = async (connectionId: string) => {
    if (selectedSession && selectedSession !== connectionId) {
      setSessionCache((prev) => ({
        ...prev,
        [selectedSession]: { conversationId, chat },
      }))
    }

    setSelectedSession(connectionId)

    const cached = sessionCache[connectionId]
    if (cached) {
      setConversationId(cached.conversationId)
      setChat(cached.chat)
      connectSessionSse(connectionId)
      if (cached.conversationId) {
        connectConversationSse(cached.conversationId)
      }
      pushLog(`restored cached session context: ${connectionId}`)
      return
    }

    setChat([{ kind: 'system', ts: now(), text: `selected session ${connectionId}` }])
    connectSessionSse(connectionId)
    const history = await api(`/api/sessions/${connectionId}/history`)
    const restoredChat: ChatItem[] = (history.messages || []).map((m: any) => {
      if (m.role === 'assistant') return { kind: 'assistant', ts: now(), text: m.content }
      if (m.role === 'user') return { kind: 'user', ts: now(), text: m.content }
      return { kind: 'system', ts: now(), text: `${m.role}: ${m.content}` }
    })
    setChat((prev) => {
      const seed = [{ kind: 'system', ts: now(), text: `selected session ${connectionId}` } as ChatItem]
      return [...seed, ...restoredChat, ...prev.filter((x) => x.kind === 'system' && x.text.includes('selected session'))]
    })

    const conv = await api('/api/conversations', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ connection_id: connectionId, title: 'webapp' }),
    })
    setConversationId(conv.conversation_id)
    localStorage.setItem('vectorshell:selectedSession', connectionId)
    connectConversationSse(conv.conversation_id)
    pushLog(`conversation auto-created: ${conv.conversation_id}`)
  }

  const sendChatMessage = async (text: string) => {
    if (!conversationId) {
      throw new Error('conversation is not ready')
    }
    await api(`/api/conversations/${conversationId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message: text }),
    })
  }

  const callTool = async (tool_name: string, args: any) => {
    if (!selectedSession) throw new Error('session not selected')
    return api(`/api/sessions/${selectedSession}/tools`, {
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
      if (!selectedSession) throw new Error('session not selected')
      await api(`/api/sessions/${selectedSession}/clean`, { method: 'POST' })
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
    if (!selectedSession) return
    setSessionCache((prev) => ({
      ...prev,
      [selectedSession]: { conversationId, chat },
    }))
  }, [selectedSession, conversationId, chat])

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
    loadSessions().catch((e) => pushLog(`load sessions error: ${String(e)}`))
    return () => {
      sessionEsRef.current?.close()
      convEsRef.current?.close()
    }
  }, [])

  useEffect(() => {
    if (!sessions.length) return
    const remembered = localStorage.getItem('vectorshell:selectedSession')
    if (!remembered) return
    if (sessions.some((s) => s.connection_id === remembered) && selectedSession !== remembered) {
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
        <h1>VectorShell</h1>
        <button className="gear" onClick={() => setShowSettings(true)}>
          ⚙️
        </button>
      </header>

      <div className="layout">
        <aside className="session-panel">
          <div className="panel-title">Sessions</div>
          {sessions.map((s) => (
            <button
              key={s.connection_id}
              className={`session-item ${selectedSession === s.connection_id ? 'active' : ''}`}
              onClick={() => onSelectSession(s.connection_id)}
            >
              <div>{s.hostname}</div>
              <small>{s.username} · {s.connection_id.slice(0, 8)}</small>
            </button>
          ))}
          <button className="refresh" onClick={() => loadSessions().catch((e) => pushLog(String(e)))}>Refresh</button>

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

          <div className="logs-toolbar">
            <select value={logFilter} onChange={(e) => setLogFilter(e.target.value as any)}>
              <option value="all">all</option>
              <option value="agent">agent</option>
              <option value="tool">tool</option>
              <option value="exec">exec</option>
            </select>
            <button onClick={() => setLogs([])}>Clear Logs</button>
          </div>
          <pre className="logs-box">{filteredLogs.join('\n')}</pre>
        </aside>

        <section className="chat-panel">
          <div className="chat-header">
            <div>conversation: {conversationId || '-'}</div>
          </div>
          <div className="chat-body" ref={chatBodyRef}>
            {chat.map((m, idx) => {
              if (m.kind === 'tool') {
                const icon = m.text.toLowerCase().includes('result') ? '✅' : '🛠️'
                return <div key={idx} className="tool-row">{icon} {m.text}</div>
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
                    <div className="meta">Exec · {m.ts} · exit={m.exitCode} · {m.durationMs}ms</div>
                    <div className="mono">$ {m.command}</div>
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

          <div className="chat-input">
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={onInputKeyDown}
              placeholder="Type message or /help /exec /upload /download"
            />
            <button onClick={() => onSubmitInput().catch((e) => setChat((p) => [...p, { kind: 'system', ts: now(), text: String(e) }]))}>
              Send
            </button>
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
          <button onClick={() => onSubmit(fileRef.current?.files?.[0] || null).catch(console.error)}>Upload</button>
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
          <button onClick={() => onSubmit().catch(console.error)}>Download</button>
        </div>
      </div>
    </div>
  )
}

export default App
