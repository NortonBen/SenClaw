import { useCallback, useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { api, connectLiveView, type TabInfo, type HistRow } from './api'

type PanelMode = 'chat' | 'act' | 'history' | 'bookmarks'
interface ChatMsg { role: 'user' | 'assistant'; content: string }

// Backend viewport (must match src/main.rs Viewport).
const VIEW_W = 1280
const VIEW_H = 800

export default function App() {
  const [addr, setAddr] = useState('')
  const [info, setInfo] = useState({ url: '', title: '' })
  const [frame, setFrame] = useState<string>('')
  const [tabs, setTabs] = useState<TabInfo[]>([])
  const [showPanel, setShowPanel] = useState(true)
  const [panel, setPanel] = useState<PanelMode>('chat')

  const wsRef = useRef<WebSocket | null>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)

  // ---- Live view WebSocket ----
  useEffect(() => {
    const ws = connectLiveView((f) => {
      setFrame(f.data)
      setInfo({ url: f.url, title: f.title })
      setAddr((cur) => (document.activeElement?.tagName === 'INPUT' ? cur : f.url))
    })
    wsRef.current = ws
    const reconnect = () => {
      setTimeout(() => {
        if (wsRef.current === ws) {
          wsRef.current = connectLiveView((f) => {
            setFrame(f.data); setInfo({ url: f.url, title: f.title })
          })
        }
      }, 1000)
    }
    ws.onclose = reconnect
    refreshTabs()
    return () => { ws.onclose = null; ws.close() }
  }, [])

  const send = (m: any) => wsRef.current?.readyState === 1 && wsRef.current.send(JSON.stringify(m))
  const refreshTabs = () => api.tabs().then((t) => setTabs(t.tabs)).catch(() => {})

  const go = (raw?: string) => {
    const url = (raw ?? addr).trim()
    if (!url) return
    send({ action: 'navigate', url })
    setTimeout(refreshTabs, 800)
  }

  // ---- Viewport interaction: map screen coords → backend viewport coords ----
  const toViewport = (e: React.MouseEvent) => {
    const img = imgRef.current!
    const rect = img.getBoundingClientRect()
    const scale = Math.min(rect.width / VIEW_W, rect.height / VIEW_H)
    const dispW = VIEW_W * scale, dispH = VIEW_H * scale
    const offX = (rect.width - dispW) / 2, offY = (rect.height - dispH) / 2
    const x = (e.clientX - rect.left - offX) / scale
    const y = (e.clientY - rect.top - offY) / scale
    return { x: Math.max(0, Math.min(VIEW_W, x)), y: Math.max(0, Math.min(VIEW_H, y)) }
  }
  const onViewClick = (e: React.MouseEvent) => {
    const { x, y } = toViewport(e)
    send({ action: 'click', x, y })
    imgRef.current?.focus()
  }
  const onViewWheel = (e: React.WheelEvent) => {
    send({ action: 'scroll', dx: e.deltaX, dy: e.deltaY })
  }
  const onViewKey = (e: React.KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey) return
    const special = ['Enter', 'Backspace', 'Delete', 'Tab', 'Escape', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown']
    if (special.includes(e.key)) { e.preventDefault(); send({ action: 'press', key: e.key }) }
    else if (e.key.length === 1) { e.preventDefault(); send({ action: 'type', text: e.key }) }
  }

  return (
    <div className="app">
      <div className="toolbar">
        <button className="nav-btn" title="Back" onClick={() => send({ action: 'back' })}>←</button>
        <button className="nav-btn" title="Forward" onClick={() => send({ action: 'forward' })}>→</button>
        <button className="nav-btn" title="Reload" onClick={() => send({ action: 'reload' })}>⟳</button>
        <form className="address" onSubmit={(e) => { e.preventDefault(); go() }}>
          <span className="lock">{info.url.startsWith('https') ? '🔒' : '🌐'}</span>
          <input
            value={addr}
            onChange={(e) => setAddr(e.target.value)}
            placeholder="Search Google or type a URL"
            spellCheck={false}
          />
        </form>
        <button className="nav-btn" title="Bookmark" onClick={() => info.url && api.addBookmark(info.url, info.title)}>☆</button>
        <button className={'pill ' + (showPanel ? 'accent' : '')} onClick={() => setShowPanel((v) => !v)}>✨ AI</button>
      </div>

      <div className="tabs">
        {tabs.map((t) => (
          <div key={t.index} className={'tab ' + (t.active ? 'active' : '')}
               onClick={() => { api.switchTab(t.index).then(refreshTabs) }}>
            <span className="tab-title">{t.title || t.url || 'New tab'}</span>
            {tabs.length > 1 && (
              <span className="x" onClick={(e) => { e.stopPropagation(); api.closeTab(t.index).then(refreshTabs) }}>✕</span>
            )}
          </div>
        ))}
        <div className="tab" onClick={() => api.newTab().then(refreshTabs)}>＋</div>
      </div>

      <div className="body">
        <div className="viewport">
          {frame ? (
            <img
              ref={imgRef}
              src={`data:image/jpeg;base64,${frame}`}
              tabIndex={0}
              onClick={onViewClick}
              onWheel={onViewWheel}
              onKeyDown={onViewKey}
              draggable={false}
              alt="page"
            />
          ) : (
            <div className="placeholder">Connecting to browser…<br />Type a URL above to start.</div>
          )}
        </div>
        {showPanel && <SidePanel panel={panel} setPanel={setPanel} pageInfo={info} onOpen={go} />}
      </div>
    </div>
  )
}

// ---------- Side panel: Chat / Act / History / Bookmarks ----------

function SidePanel({ panel, setPanel, pageInfo, onOpen }: {
  panel: PanelMode; setPanel: (p: PanelMode) => void
  pageInfo: { url: string; title: string }; onOpen: (u: string) => void
}) {
  return (
    <div className="panel-side">
      <div className="panel-tabs">
        {(['chat', 'act', 'history', 'bookmarks'] as PanelMode[]).map((p) => (
          <div key={p} className={'panel-tab ' + (panel === p ? 'active' : '')} onClick={() => setPanel(p)}>
            {p === 'chat' ? '💬 Chat' : p === 'act' ? '▶ Act' : p === 'history' ? '🕘 History' : '⭐ Marks'}
          </div>
        ))}
      </div>
      {panel === 'chat' && <ChatPanel pageInfo={pageInfo} />}
      {panel === 'act' && <ActPanel />}
      {panel === 'history' && <ListPanel load={api.history} onOpen={onOpen} />}
      {panel === 'bookmarks' && <ListPanel load={api.bookmarks} onOpen={onOpen} />}
    </div>
  )
}

function ChatPanel({ pageInfo }: { pageInfo: { url: string; title: string } }) {
  const [msgs, setMsgs] = useState<ChatMsg[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const endRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => { endRef.current?.scrollIntoView({ behavior: 'smooth' }) }, [msgs, busy])

  const sendMsg = async () => {
    const text = input.trim()
    if (!text || busy) return
    const next = [...msgs, { role: 'user' as const, content: text }]
    setMsgs(next); setInput(''); setBusy(true)
    try {
      // Ground the answer in the current page snapshot.
      let ctx = `${pageInfo.title} — ${pageInfo.url}`
      try {
        const snap = await api.snapshot()
        ctx = `Title: ${snap.title}\nURL: ${snap.url}\n\n${snap.text?.slice(0, 3000) || ''}`
      } catch {}
      const r = await api.chat(next, ctx)
      setMsgs((m) => [...m, { role: 'assistant', content: r.answer }])
    } catch (e: any) {
      setMsgs((m) => [...m, { role: 'assistant', content: '⚠️ ' + (e.message || 'error') }])
    } finally { setBusy(false) }
  }

  return (
    <>
      <div className="panel-content">
        {msgs.length === 0 && <div className="hint">Ask about this page — summarize it, find something, or plan next steps. The AI sees the current page.</div>}
        {msgs.map((m, i) => (
          <div key={i} className={'msg ' + m.role}>
            <div className="who">{m.role === 'user' ? 'You' : 'SenClaw Browser'}</div>
            <div className="bubble"><ReactMarkdown remarkPlugins={[remarkGfm]}>{m.content}</ReactMarkdown></div>
          </div>
        ))}
        {busy && <div className="msg assistant"><div className="who">SenClaw Browser</div><div className="bubble"><span className="spinner" /> thinking…</div></div>}
        <div ref={endRef} />
      </div>
      <div className="composer">
        <textarea value={input} onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMsg() } }}
          placeholder="Ask about this page…" />
        <div className="composer-row"><button className="pill accent" onClick={sendMsg} disabled={busy}>Send</button></div>
      </div>
    </>
  )
}

function ActPanel() {
  const [goal, setGoal] = useState('')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<any>(null)

  const run = async () => {
    const g = goal.trim()
    if (!g || busy) return
    setBusy(true); setResult(null)
    try { setResult(await api.act(g)) }
    catch (e: any) { setResult({ error: e.message || 'error' }) }
    finally { setBusy(false) }
  }

  return (
    <>
      <div className="panel-content">
        <div className="hint">Give the AI a goal on the live page — it observes, decides and acts step by step (search, click, fill forms). Human-like input, indistinguishable from a person.</div>
        {busy && <div className="act-step"><span className="spinner" /> working on it…</div>}
        {result?.error && <div className="act-step" style={{ color: 'var(--danger)' }}>⚠️ {result.error}</div>}
        {result?.steps?.map((s: any, i: number) => (
          <div key={i} className="act-step">
            <span className="k">{s.action}</span>
            {s.reason && <span className="badge">{String(s.reason)}</span>}
            {s.detail?.reason && <div style={{ color: 'var(--muted)', marginTop: 4 }}>{s.detail.reason}</div>}
            {s.result?.error && <div style={{ color: 'var(--danger)', marginTop: 4 }}>{s.result.error}</div>}
          </div>
        ))}
        {result?.final && <div className="hint">✓ Ended at: {result.final.title} — {result.final.url}</div>}
      </div>
      <div className="composer">
        <textarea value={goal} onChange={(e) => setGoal(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); run() } }}
          placeholder="e.g. search Wikipedia for the Eiffel Tower and open the article" />
        <div className="composer-row"><button className="pill accent" onClick={run} disabled={busy}>▶ Run</button></div>
      </div>
    </>
  )
}

function ListPanel({ load, onOpen }: { load: () => Promise<HistRow[]>; onOpen: (u: string) => void }) {
  const [rows, setRows] = useState<HistRow[]>([])
  useEffect(() => { load().then(setRows).catch(() => setRows([])) }, [])
  return (
    <div className="panel-content">
      {rows.length === 0 && <div className="hint">Nothing here yet.</div>}
      {rows.map((r) => (
        <div key={r.id} className="list-item" onClick={() => onOpen(r.url)}>
          <div>{r.title || r.url}</div>
          <div className="u">{r.url}</div>
        </div>
      ))}
    </div>
  )
}
