import { useCallback, useEffect, useRef, useState } from 'react'
import { api, subscribeEvents } from './api'
import type { DiagramMeta, EditorStatus, Kind } from './api'
import { DrawioFrame } from './DrawioFrame'
import type { DrawioFrameHandle } from './DrawioFrame'

const KINDS: { value: Kind; label: string }[] = [
  { value: 'flowchart', label: 'Lưu đồ' },
  { value: 'sequence', label: 'Sequence' },
  { value: 'architecture', label: 'Kiến trúc' },
  { value: 'er', label: 'ER (dữ liệu)' },
  { value: 'state', label: 'State machine' },
  { value: 'class', label: 'Class' },
  { value: 'org', label: 'Tổ chức' },
  { value: 'network', label: 'Mạng' },
  { value: 'bpmn', label: 'BPMN' },
]

function fmtBytes(n?: number): string {
  if (!n) return '0 MB'
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

function fmtTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString('vi-VN', {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

interface Current {
  id: number
  name: string
  xml: string
}

export default function App() {
  const [editor, setEditor] = useState<EditorStatus>({ status: 'missing' })
  const [diagrams, setDiagrams] = useState<DiagramMeta[]>([])
  const [current, setCurrent] = useState<Current | null>(null)
  const [dark, setDark] = useState(
    () => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false,
  )
  const [aiOpen, setAiOpen] = useState(true)

  // AI panel state
  const [prompt, setPrompt] = useState('')
  const [kind, setKind] = useState<Kind>('flowchart')
  const [mode, setMode] = useState<'mermaid' | 'xml'>('mermaid')
  const [target, setTarget] = useState<'new' | 'replace' | 'merge'>('new')
  const [instruction, setInstruction] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const frame = useRef<DrawioFrameHandle>(null)
  const currentIdRef = useRef<number | null>(null)
  currentIdRef.current = current?.id ?? null
  /** AI content to apply once a freshly-mounted editor reports loaded. */
  const pending = useRef<{ kind: 'mermaid' | 'xml'; content: string } | null>(null)
  /** Suppress SSE-triggered editor reloads right after our own mutations. */
  const suppressUntil = useRef(0)
  const saveTimer = useRef<number | undefined>(undefined)
  const svgTimer = useRef<number | undefined>(undefined)

  // ---- SenClaw host handshake (theme) ----
  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      const d = e.data as { type?: string; theme?: string; env?: { theme?: string } }
      if (!d || typeof d !== 'object' || typeof d.type !== 'string') return
      if (d.type === 'senclaw:init' || d.type === 'senclaw:theme') {
        const theme = d.theme ?? d.env?.theme
        if (theme === 'dark') setDark(true)
        if (theme === 'light') setDark(false)
      }
    }
    window.addEventListener('message', onMsg)
    window.parent?.postMessage({ type: 'senclaw:ready' }, '*')
    return () => window.removeEventListener('message', onMsg)
  }, [])
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
  }, [dark])

  // ---- Editor availability (first-run download) ----
  const refreshStatus = useCallback(async () => {
    try {
      setEditor((await api.status()).editor)
    } catch {
      /* backend restarting */
    }
  }, [])
  useEffect(() => {
    refreshStatus()
  }, [refreshStatus])
  useEffect(() => {
    if (editor.status === 'ready') return
    const t = window.setTimeout(refreshStatus, 1000)
    return () => window.clearTimeout(t)
  }, [editor, refreshStatus])

  // ---- Diagram list / selection ----
  const refreshList = useCallback(async () => {
    try {
      setDiagrams(await api.list())
    } catch {
      /* transient */
    }
  }, [])
  useEffect(() => {
    refreshList()
  }, [refreshList])

  const select = useCallback(async (id: number) => {
    try {
      const d = await api.get(id)
      setCurrent({ id: d.id, name: d.name, xml: d.xml })
    } catch (e) {
      setError((e as Error).message)
    }
  }, [])

  // Deep link ?d=<id>
  useEffect(() => {
    const d = new URLSearchParams(window.location.search).get('d')
    if (d && Number(d) > 0) select(Number(d))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // ---- Live updates from MCP / other writers ----
  useEffect(
    () =>
      subscribeEvents((ev) => {
        refreshList()
        if (Date.now() < suppressUntil.current) return
        if (ev.type === 'diagram:update' && ev.id === currentIdRef.current) {
          api
            .get(ev.id)
            .then((d) => {
              setCurrent({ id: d.id, name: d.name, xml: d.xml })
              frame.current?.loadXml(d.xml)
            })
            .catch(() => {})
        }
        if (ev.type === 'diagram:delete' && ev.id === currentIdRef.current) {
          setCurrent(null)
        }
      }),
    [refreshList],
  )

  // ---- Editor callbacks ----
  const onAutosave = useCallback(
    (xml: string) => {
      const id = currentIdRef.current
      if (!id) return
      window.clearTimeout(saveTimer.current)
      saveTimer.current = window.setTimeout(() => {
        suppressUntil.current = Date.now() + 1500
        api.putXml(id, xml).then(refreshList).catch(() => {})
      }, 600)
      // Throttled SVG snapshot so headless exports stay reasonably fresh.
      if (svgTimer.current === undefined) {
        svgTimer.current = window.setTimeout(() => {
          svgTimer.current = undefined
          frame.current?.exportSvg()
        }, 8000)
      }
    },
    [refreshList],
  )

  const onSvg = useCallback((svg: string) => {
    const id = currentIdRef.current
    if (id) api.putSvg(id, svg).catch(() => {})
  }, [])

  const onFrameReady = useCallback(() => {
    const p = pending.current
    if (!p) return
    pending.current = null
    if (p.kind === 'mermaid') frame.current?.loadMermaid(p.content)
    else frame.current?.loadXml(p.content)
  }, [])

  // ---- Diagram CRUD ----
  const createDiagram = useCallback(async () => {
    const name = window.prompt('Tên sơ đồ mới:', 'Sơ đồ mới')
    if (name === null) return
    suppressUntil.current = Date.now() + 1500
    const { id } = await api.create(name.trim() || 'Sơ đồ mới')
    await refreshList()
    await select(id)
  }, [refreshList, select])

  const renameCurrent = useCallback(async () => {
    if (!current) return
    const name = window.prompt('Đổi tên sơ đồ:', current.name)
    if (!name || !name.trim()) return
    suppressUntil.current = Date.now() + 1500
    await api.rename(current.id, name.trim())
    setCurrent({ ...current, name: name.trim() })
    refreshList()
  }, [current, refreshList])

  const deleteDiagram = useCallback(
    async (id: number, name: string) => {
      if (!window.confirm(`Xoá sơ đồ "${name}"?`)) return
      suppressUntil.current = Date.now() + 1500
      await api.remove(id)
      if (currentIdRef.current === id) setCurrent(null)
      refreshList()
    },
    [refreshList],
  )

  // ---- AI actions ----
  const runGenerate = useCallback(async () => {
    if (!prompt.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      const effTarget = current ? target : 'new'
      const res = await api.generate({
        prompt: prompt.trim(),
        kind,
        mode,
        diagram_id: current?.id,
      })
      const isXml = res.mode === 'xml'
      const content = (isXml ? res.xml : res.mermaid) ?? ''
      if (!content) throw new Error('AI trả về nội dung rỗng')
      if (effTarget === 'new') {
        const name = prompt.trim().split('\n')[0].slice(0, 48) || 'Sơ đồ AI'
        suppressUntil.current = Date.now() + 1500
        const { id } = await api.create(name, kind)
        pending.current = { kind: isXml ? 'xml' : 'mermaid', content }
        await select(id)
        refreshList()
      } else if (effTarget === 'merge' && isXml) {
        frame.current?.mergeXml(content)
      } else if (isXml) {
        frame.current?.loadXml(content)
      } else {
        frame.current?.loadMermaid(content)
      }
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }, [prompt, kind, mode, target, current, busy, select, refreshList])

  const runEdit = useCallback(async () => {
    if (!current || !instruction.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      const res = await api.edit({
        diagram_id: current.id,
        xml: frame.current?.getXml() || current.xml,
        instruction: instruction.trim(),
      })
      frame.current?.loadXml(res.xml)
      setInstruction('')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }, [current, instruction, busy])

  // ---- Render ----
  const editorReady = editor.status === 'ready'

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="sidebar-head">
          <h1>📐 Diagrams</h1>
          <button className="btn primary" onClick={createDiagram}>
            ＋ Mới
          </button>
        </div>
        <div className="diagram-list">
          {diagrams.length === 0 && <div className="empty">Chưa có sơ đồ nào — tạo mới hoặc dùng ✨ AI.</div>}
          {diagrams.map((d) => (
            <div
              key={d.id}
              className={`diagram-item ${current?.id === d.id ? 'active' : ''}`}
              onClick={() => select(d.id)}
            >
              <div className="diagram-name">{d.name}</div>
              <div className="diagram-meta">
                <span className="badge">{d.kind}</span>
                <span>{d.cells} ô</span>
                <span>{fmtTime(d.updated_at)}</span>
                <button
                  className="icon-btn"
                  title="Xoá"
                  onClick={(e) => {
                    e.stopPropagation()
                    deleteDiagram(d.id, d.name)
                  }}
                >
                  🗑
                </button>
              </div>
            </div>
          ))}
        </div>
      </aside>

      <main className="main">
        <div className="toolbar">
          {current ? (
            <>
              <span className="current-name" onDoubleClick={renameCurrent} title="Nhấp đúp để đổi tên">
                {current.name}
              </span>
              <button className="btn" onClick={renameCurrent}>
                ✎
              </button>
              <a className="btn" href={`/api/diagrams/${current.id}/export?format=xml`} target="_blank" rel="noreferrer">
                ⬇ .drawio
              </a>
              <a className="btn" href={`/api/diagrams/${current.id}/export?format=svg`} target="_blank" rel="noreferrer">
                ⬇ SVG
              </a>
            </>
          ) : (
            <span className="current-name muted">Chọn hoặc tạo một sơ đồ</span>
          )}
          <div className="spacer" />
          <button className={`btn ${aiOpen ? 'primary' : ''}`} onClick={() => setAiOpen(!aiOpen)}>
            ✨ AI
          </button>
        </div>

        <div className="editor-area">
          {!editorReady ? (
            <div className="editor-status">
              {editor.status === 'downloading' && (
                <>
                  <h2>Đang tải trình vẽ draw.io…</h2>
                  <div className="progress">
                    <div className="progress-bar" style={{ width: `${editor.percent ?? 0}%` }} />
                  </div>
                  <p>
                    {fmtBytes(editor.received)} / {fmtBytes(editor.total)} ({editor.percent ?? 0}%) — chỉ tải một lần,
                    sau đó hoạt động offline.
                  </p>
                </>
              )}
              {editor.status === 'extracting' && <h2>Đang giải nén trình vẽ…</h2>}
              {editor.status === 'missing' && <h2>Đang chuẩn bị trình vẽ…</h2>}
              {editor.status === 'error' && (
                <>
                  <h2>Lỗi tải trình vẽ</h2>
                  <p className="error">{editor.message}</p>
                  <button
                    className="btn primary"
                    onClick={() => api.editorRetry().then((s) => setEditor(s.editor))}
                  >
                    Thử lại
                  </button>
                </>
              )}
            </div>
          ) : current ? (
            <DrawioFrame
              key={current.id}
              ref={frame}
              initialXml={current.xml}
              dark={dark}
              onAutosave={onAutosave}
              onSvg={onSvg}
              onReady={onFrameReady}
            />
          ) : (
            <div className="editor-status">
              <h2>📐 SenClaw Diagrams</h2>
              <p>Chọn một sơ đồ bên trái, tạo mới, hoặc mô tả để ✨ AI vẽ giúp bạn.</p>
            </div>
          )}
        </div>
      </main>

      {aiOpen && (
        <aside className="ai-panel">
          <h2>✨ AI vẽ sơ đồ</h2>
          <label>Mô tả</label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="VD: quy trình đăng ký tài khoản với xác thực OTP, có xử lý lỗi"
            rows={4}
          />
          <div className="row">
            <div className="field">
              <label>Loại sơ đồ</label>
              <select value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
                {KINDS.map((k) => (
                  <option key={k.value} value={k.value}>
                    {k.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Chế độ</label>
              <select value={mode} onChange={(e) => setMode(e.target.value as 'mermaid' | 'xml')}>
                <option value="mermaid">Nhanh (Mermaid)</option>
                <option value="xml">Chi tiết (XML)</option>
              </select>
            </div>
          </div>
          {current && (
            <div className="field">
              <label>Đích</label>
              <select value={target} onChange={(e) => setTarget(e.target.value as typeof target)}>
                <option value="new">Sơ đồ mới</option>
                <option value="replace">Thay thế sơ đồ hiện tại</option>
                {mode === 'xml' && <option value="merge">Thêm vào sơ đồ hiện tại</option>}
              </select>
            </div>
          )}
          <button className="btn primary wide" onClick={runGenerate} disabled={busy || !prompt.trim()}>
            {busy ? 'Đang vẽ…' : 'Vẽ sơ đồ'}
          </button>

          {current && (
            <>
              <hr />
              <h2>🛠 Sửa bằng AI</h2>
              <textarea
                value={instruction}
                onChange={(e) => setInstruction(e.target.value)}
                placeholder="VD: thêm bước xác thực OTP sau khi đăng nhập"
                rows={3}
              />
              <button className="btn primary wide" onClick={runEdit} disabled={busy || !instruction.trim()}>
                {busy ? 'Đang sửa…' : 'Áp dụng'}
              </button>
            </>
          )}

          {error && <div className="error">{error}</div>}
          <p className="hint">
            Chế độ Nhanh dùng Mermaid (rẻ, ổn định); Chi tiết sinh XML mxGraph (kiểm soát bố cục/màu). Sơ đồ AI vẽ
            luôn chỉnh sửa được trực tiếp trên canvas.
          </p>
        </aside>
      )}
    </div>
  )
}
