import { useCallback, useEffect, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { DocxEditor } from '@eigenpal/docx-editor-react'
import '@eigenpal/docx-editor-react/styles.css'
import { api, type DocMeta } from './api'
import AgentPanel from './AgentPanel'

type SaveState = 'saved' | 'dirty' | 'saving' | 'error' | 'idle'
type Theme = 'light' | 'dark'

const THEME_KEY = 'docx-editor.theme'

function getInitialTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_KEY) as Theme | null
    if (stored === 'light' || stored === 'dark') return stored
  } catch { /* ignore */ }
  if (window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches) return 'light'
  return 'dark'
}

export default function App() {
  const [docs, setDocs] = useState<DocMeta[]>([])
  const [activeId, setActiveId] = useState<number | null>(null)
  const [activeTitle, setActiveTitle] = useState('')
  const [buffer, setBuffer] = useState<ArrayBuffer | null>(null)
  const [saveState, setSaveState] = useState<SaveState>('idle')
  const [toast, setToast] = useState<{ msg: string; kind: 'error' | 'success' } | null>(null)
  const [theme, setTheme] = useState<Theme>(getInitialTheme)
  const [agentOpen, setAgentOpen] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const dirtyRef = useRef(false)
  const savingRef = useRef(false)
  const docTextRef = useRef('')

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    try { localStorage.setItem(THEME_KEY, theme) } catch { /* ignore */ }
  }, [theme])

  const showToast = useCallback((msg: string, kind: 'error' | 'success' = 'success') => {
    setToast({ msg, kind })
    setTimeout(() => setToast(null), 2500)
  }, [])

  const refresh = useCallback(async () => {
    try {
      const { docs } = await api.list()
      setDocs(docs)
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [showToast])

  useEffect(() => { refresh() }, [refresh])

  const openDoc = useCallback(async (id: number, title: string) => {
    try {
      const [rawRes, docRes] = await Promise.all([
        fetch(`/api/doc/${id}/raw`),
        api.get(id),
      ])
      if (!rawRes.ok) throw new Error(`${rawRes.status} ${rawRes.statusText}`)
      const buf = await rawRes.arrayBuffer()
      setActiveId(id)
      setActiveTitle(title)
      setBuffer(buf)
      docTextRef.current = docRes.doc.content_text
      setSaveState('saved')
      dirtyRef.current = false
      setSidebarOpen(false)
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [showToast])

  const closeDoc = useCallback(() => {
    setActiveId(null)
    setBuffer(null)
    setActiveTitle('')
    setSaveState('idle')
    dirtyRef.current = false
    setSidebarOpen(true)
  }, [])

  const createNew = useCallback(async () => {
    try {
      const { id } = await api.create('Tài liệu mới', '')
      await refresh()
      const { doc } = await api.get(id)
      await openDoc(id, doc.title)
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [refresh, openDoc, showToast])

  const uploadFile = useCallback(async (file: File) => {
    if (!file.name.toLowerCase().endsWith('.docx')) {
      showToast('Chỉ hỗ trợ file .docx', 'error')
      return
    }
    try {
      const { id, title, chars } = await api.upload(file)
      showToast(`Đã mở "${title}" (${chars} ký tự)`)
      await refresh()
      await openDoc(id, title)
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [refresh, openDoc, showToast])

  const deleteDoc = useCallback(async (id: number) => {
    if (!confirm('Xoá tài liệu này?')) return
    try {
      await api.delete(id)
      if (activeId === id) {
        setActiveId(null); setBuffer(null); setActiveTitle('')
        setSidebarOpen(true)
      }
      await refresh()
      showToast('Đã xoá')
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [activeId, refresh, showToast])

  const doSaveBuffer = useCallback(async (id: number, buf: ArrayBuffer) => {
    if (savingRef.current) return
    savingRef.current = true
    setSaveState('saving')
    try {
      const res = await fetch(`/api/doc/${id}/raw`, {
        method: 'PUT',
        headers: {
          'Content-Type':
            'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        },
        body: buf,
      })
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
      setSaveState('saved')
      dirtyRef.current = false
      // Refresh text projection for the agent panel.
      const { doc } = await api.get(id)
      docTextRef.current = doc.content_text
      await refresh()
    } catch (e) {
      setSaveState('error')
      showToast(String(e), 'error')
    } finally {
      savingRef.current = false
    }
  }, [refresh, showToast])

  const onEditorSave = useCallback((buf: ArrayBuffer) => {
    if (!activeId) return
    doSaveBuffer(activeId, buf)
  }, [activeId, doSaveBuffer])

  const onEditorChange = useCallback(() => {
    if (!dirtyRef.current) {
      dirtyRef.current = true
      setSaveState('dirty')
    }
  }, [])

  // Poll for external edits (e.g. AI Agent via MCP) — only when clean.
  useEffect(() => {
    if (!activeId) return
    const iv = window.setInterval(async () => {
      if (dirtyRef.current || savingRef.current) return
      try {
        const { doc } = await api.get(activeId)
        const known = docs.find(d => d.id === activeId)?.updated_at ?? 0
        if (doc.updated_at > known + 1) {
          const res = await fetch(`/api/doc/${activeId}/raw`)
          if (res.ok) {
            const buf = await res.arrayBuffer()
            setBuffer(buf)
            setActiveTitle(doc.title)
            docTextRef.current = doc.content_text
            await refresh()
          }
        }
      } catch { /* silent */ }
    }, 4000)
    return () => window.clearInterval(iv)
  }, [activeId, docs, refresh])

  const applyRewrite = useCallback(async (text: string) => {
    if (!activeId) return
    try {
      await fetch('/api/chat/apply', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id: activeId, content: text }),
      })
      // Reload the buffer so the editor shows the new text.
      const res = await fetch(`/api/doc/${activeId}/raw`)
      if (res.ok) {
        setBuffer(await res.arrayBuffer())
        docTextRef.current = text
      }
      showToast('Đã áp dụng bản viết lại')
      await refresh()
    } catch (e) {
      showToast(String(e), 'error')
    }
  }, [activeId, refresh, showToast])

  const statusLabel = (() => {
    switch (saveState) {
      case 'saving': return 'Đang lưu…'
      case 'saved':  return 'Đã lưu'
      case 'dirty':  return 'Chưa lưu · Ctrl/⌘+S'
      case 'error':  return 'Lỗi lưu'
      default:       return ''
    }
  })()

  return (
    <div className={`app${sidebarOpen ? '' : ' sidebar-collapsed'}`}>
      <aside className="sidebar">
        <div className="sidebar-header">
          <h1>
            <span>📄 DOCX Editor</span>
            <button
              className="theme-toggle"
              onClick={() => setTheme(t => (t === 'dark' ? 'light' : 'dark'))}
              title="Đổi giao diện sáng / tối"
            >
              {theme === 'dark' ? '☀' : '🌙'}
            </button>
          </h1>
          <div className="subtitle">Soạn thảo & chia sẻ với AI Agent</div>
          <div className="sidebar-actions">
            <button className="primary" onClick={createNew}>+ Tạo mới</button>
            <button onClick={() => fileInputRef.current?.click()}>⬆ Tải lên</button>
          </div>
          <input
            ref={fileInputRef}
            type="file"
            accept=".docx"
            style={{ display: 'none' }}
            onChange={e => {
              const f = e.target.files?.[0]
              if (f) uploadFile(f)
              e.target.value = ''
            }}
          />
        </div>
        <div className="doc-list">
          {docs.length === 0 && <div className="doc-empty">Chưa có tài liệu.<br/>Bấm "Tạo mới" hoặc tải lên .docx.</div>}
          {docs.map(d => (
            <SidebarDocItem
              key={d.id}
              doc={d}
              active={d.id === activeId}
              onOpen={() => openDoc(d.id, d.title)}
              onRename={next => {
                if (d.id === activeId) setActiveTitle(next)
                api.rename(d.id, next).then(refresh).catch(e => showToast(String(e), 'error'))
              }}
              onDelete={() => deleteDoc(d.id)}
            />
          ))}
        </div>
      </aside>

      <main className="editor">
        {activeId && buffer ? (
          <>
            <div className="editor-overlay">
              <button
                className="icon-btn"
                onClick={() => setSidebarOpen(o => !o)}
                title={sidebarOpen ? 'Ẩn danh sách tài liệu' : 'Hiện danh sách tài liệu'}
              >
                {sidebarOpen ? '⇤' : '☰'}
              </button>
              <button
                className="icon-btn"
                onClick={closeDoc}
                title="Đóng tài liệu"
              >
                ✕
              </button>
              <a href={api.downloadUrl(activeId)} download title="Tải file .docx về máy">
                <button className="icon-btn">⬇</button>
              </a>
              <span className={`status-pill ${saveState}`} title={activeTitle}>
                {statusLabel || 'Đã lưu'}
              </span>
            </div>
            <div className="editor-wysiwyg">
              <DocxEditor
                key={activeId}
                documentBuffer={buffer}
                onSave={onEditorSave}
                onChange={onEditorChange}
                author="Bạn"
                colorMode={theme}
                agentPanel={{
                  open: agentOpen,
                  onOpenChange: setAgentOpen,
                  title: 'AI Agent',
                  defaultWidth: 380,
                  render: ({ close }) => (
                    <AgentPanel
                      docId={activeId!}
                      getDocText={() => docTextRef.current}
                      onApplyRewrite={applyRewrite}
                      close={close}
                    />
                  ),
                }}
              />
            </div>
          </>
        ) : (
          <div className="placeholder-panel">
            <h2>Chưa mở tài liệu nào</h2>
            <p>
              Tạo tài liệu mới, tải lên một file <code>.docx</code>, hoặc yêu cầu
              AI Agent tạo hộ. Bấm ✨ trên toolbar để mở panel Agent — hỏi về nội
              dung, xin phản hồi, hoặc yêu cầu viết lại toàn văn (có nút Áp dụng).
              Toolbar hỗ trợ đầy đủ heading, in đậm/nghiêng/gạch chân, màu chữ,
              highlight, bảng, danh sách, link, chú thích, track-changes, comments,
              tìm-thay-thế.
            </p>
          </div>
        )}
      </main>

      {toast && <div className={`toast ${toast.kind}`}>{toast.msg}</div>}
    </div>
  )
}

function SidebarDocItem({
  doc,
  active,
  onOpen,
  onRename,
  onDelete,
}: {
  doc: DocMeta
  active: boolean
  onOpen: () => void
  onRename: (next: string) => void
  onDelete: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(doc.title)

  const commit = () => {
    const next = draft.trim()
    setEditing(false)
    if (next && next !== doc.title) onRename(next)
    else setDraft(doc.title)
  }
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') { e.preventDefault(); commit() }
    else if (e.key === 'Escape') { e.preventDefault(); setDraft(doc.title); setEditing(false) }
  }

  return (
    <div
      className={`doc-item${active ? ' active' : ''}`}
      onClick={editing ? undefined : onOpen}
    >
      <div className="doc-item-head">
        {editing ? (
          <input
            className="title-edit"
            type="text"
            value={draft}
            autoFocus
            onClick={e => e.stopPropagation()}
            onChange={e => setDraft(e.target.value)}
            onKeyDown={onKey}
            onBlur={commit}
          />
        ) : (
          <div
            className="title"
            title="Nhấn đúp để đổi tên"
            onDoubleClick={e => { e.stopPropagation(); setDraft(doc.title); setEditing(true) }}
          >
            {doc.title}
          </div>
        )}
        <button
          className="doc-trash"
          title="Xoá tài liệu"
          onClick={e => { e.stopPropagation(); onDelete() }}
        >
          🗑
        </button>
      </div>
      <div className="meta">
        {formatDate(doc.updated_at)} · {formatBytes(doc.size_bytes)}
      </div>
      {doc.excerpt && <div className="excerpt">{doc.excerpt}</div>}
    </div>
  )
}

function formatDate(ts: number): string {
  if (!ts) return ''
  return new Date(ts * 1000).toLocaleString('vi-VN', { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit' })
}
function formatBytes(b: number): string {
  if (!b) return '—'
  if (b < 1024) return `${b} B`
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`
  return `${(b / 1024 / 1024).toFixed(2)} MB`
}
