import { useEffect, useMemo, useState } from 'react'
import { api, type DraftSkill, type Inventory } from './api'

type Tab = 'skills' | 'subagents' | 'mcp'
type Theme = 'light' | 'dark'

function initialTheme(): Theme {
  const saved = localStorage.getItem('sb-theme')
  if (saved === 'light' || saved === 'dark') return saved
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(initialTheme)
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem('sb-theme', theme)
  }, [theme])
  const [requirement, setRequirement] = useState('')
  const [whenToRun, setWhenToRun] = useState('')
  const [inv, setInv] = useState<Inventory | null>(null)
  const [tab, setTab] = useState<Tab>('mcp')
  const [draft, setDraft] = useState<DraftSkill | null>(null)
  const [busy, setBusy] = useState<'idle' | 'gen' | 'install'>('idle')
  const [overwrite, setOverwrite] = useState(false)
  const [toast, setToast] = useState<{ kind: 'ok' | 'err'; msg: string } | null>(null)

  const loadInv = () => api.inventory().then(setInv).catch(() => {})
  useEffect(() => {
    loadInv()
  }, [])

  const notify = (kind: 'ok' | 'err', msg: string) => {
    setToast({ kind, msg })
    setTimeout(() => setToast(null), 4200)
  }

  async function generate() {
    if (!requirement.trim()) return
    setBusy('gen')
    setDraft(null)
    try {
      const d = await api.generate(requirement, whenToRun)
      setDraft(d)
      setOverwrite(false)
    } catch (e) {
      notify('err', `Không tạo được bản nháp: ${(e as Error).message}`)
    } finally {
      setBusy('idle')
    }
  }

  async function install() {
    if (!draft) return
    setBusy('install')
    try {
      await api.install({
        name: draft.name,
        description: draft.description,
        content: draft.content,
        triggers: draft.triggers,
        overwrite,
      })
      notify('ok', `Đã cài skill "${draft.name}" vào SenClaw ✓`)
      setDraft(null)
      setRequirement('')
      setWhenToRun('')
      loadInv()
    } catch (e) {
      notify('err', `Cài đặt thất bại: ${(e as Error).message}`)
    } finally {
      setBusy('idle')
    }
  }

  async function remove(name: string) {
    if (!confirm(`Gỡ skill "${name}"?`)) return
    try {
      await api.remove(name)
      notify('ok', `Đã gỡ "${name}"`)
      loadInv()
    } catch (e) {
      notify('err', (e as Error).message)
    }
  }

  return (
    <div className="app">
      <header className="hdr">
        <div className="brand">
          <span className="logo">🛠️</span>
          <div>
            <h1>SenClaw Skill Builder</h1>
            <p className="sub">Lò rèn kỹ năng — mô tả yêu cầu, AI tạo skill mới từ những công cụ bạn đang có</p>
          </div>
        </div>
        <button
          className="theme-toggle"
          onClick={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}
          title={theme === 'dark' ? 'Chuyển sang giao diện sáng' : 'Chuyển sang giao diện tối'}
          aria-label="Đổi giao diện sáng/tối"
        >
          {theme === 'dark' ? '☀️' : '🌙'}
        </button>
      </header>

      <main className="grid">
        {/* LEFT: requirement + draft */}
        <section className="col">
          <div className="card">
            <h2>1 · Yêu cầu skill</h2>
            <label>Kỹ năng dùng để làm gì?</label>
            <textarea
              value={requirement}
              onChange={(e) => setRequirement(e.target.value)}
              placeholder="VD: Tóm tắt các email chưa đọc quan trọng mỗi sáng và gửi cho tôi qua Telegram…"
              rows={4}
            />
            <label>Khi nào nên chạy / tự động kích hoạt? (tuỳ chọn)</label>
            <textarea
              value={whenToRun}
              onChange={(e) => setWhenToRun(e.target.value)}
              placeholder="VD: khi tôi nói 'điểm tin email', mỗi 8h sáng, hoặc khi hỏi về hộp thư…"
              rows={3}
            />
            <button className="primary" disabled={busy !== 'idle' || !requirement.trim()} onClick={generate}>
              {busy === 'gen' ? 'Đang phân tích & tạo…' : '✨ Phân tích & Tạo skill'}
            </button>
            <p className="hint">
              AI đọc danh sách skill, sub-agent và MCP hiện có (bên phải) để tái sử dụng công cụ sẵn có và tránh trùng lặp.
            </p>
          </div>

          {draft && (
            <div className="card draft">
              <h2>2 · Bản nháp skill</h2>

              <label>Tên (slug)</label>
              <input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />

              <label>Mô tả (dùng để khớp / "Use when…")</label>
              <textarea
                value={draft.description}
                onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                rows={3}
              />

              <label>Triggers — tự động nạp skill khi câu người dùng khớp</label>
              <TriggerEditor
                triggers={draft.triggers}
                onChange={(triggers) => setDraft({ ...draft, triggers })}
              />

              {(draft.uses_mcp?.length > 0 || draft.uses_subagents?.length > 0) && (
                <div className="uses">
                  {draft.uses_mcp?.map((t) => (
                    <span key={t} className="pill mcp">🔌 {t}</span>
                  ))}
                  {draft.uses_subagents?.map((t) => (
                    <span key={t} className="pill agent">🤖 {t}</span>
                  ))}
                </div>
              )}

              {draft.rationale && (
                <div className="rationale">
                  <strong>Lý do thiết kế:</strong> {draft.rationale}
                  {draft.model && <span className="model"> · {draft.model}</span>}
                </div>
              )}

              <label>Nội dung SKILL.md</label>
              <textarea
                className="body"
                value={draft.content}
                onChange={(e) => setDraft({ ...draft, content: e.target.value })}
                rows={14}
              />

              <div className="actions">
                <label className="chk">
                  <input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} />
                  Ghi đè nếu đã tồn tại
                </label>
                <button className="ghost" onClick={() => setDraft(null)}>Huỷ</button>
                <button className="primary" disabled={busy !== 'idle'} onClick={install}>
                  {busy === 'install' ? 'Đang cài…' : '⬇️ Cài vào SenClaw'}
                </button>
              </div>
            </div>
          )}
        </section>

        {/* RIGHT: inventory */}
        <section className="col">
          <div className="card inv">
            <div className="tabs">
              <button className={tab === 'mcp' ? 'on' : ''} onClick={() => setTab('mcp')}>
                MCP {inv ? `(${inv.mcpServers.length})` : ''}
              </button>
              <button className={tab === 'subagents' ? 'on' : ''} onClick={() => setTab('subagents')}>
                Sub-agents {inv ? `(${inv.subagents.length})` : ''}
              </button>
              <button className={tab === 'skills' ? 'on' : ''} onClick={() => setTab('skills')}>
                Skills {inv ? `(${inv.skills.length})` : ''}
              </button>
              <button className="refresh" title="Tải lại" onClick={loadInv}>↻</button>
            </div>

            {!inv && <p className="muted">Đang tải kho công cụ…</p>}
            {inv && <InventoryView inv={inv} tab={tab} onRemove={remove} />}
          </div>
        </section>
      </main>

      {toast && <div className={`toast ${toast.kind}`}>{toast.msg}</div>}
    </div>
  )
}

function TriggerEditor({ triggers, onChange }: { triggers: string[]; onChange: (t: string[]) => void }) {
  const [text, setText] = useState('')
  const add = () => {
    const v = text.trim()
    if (v && !triggers.includes(v)) onChange([...triggers, v])
    setText('')
  }
  return (
    <div className="trig">
      <div className="chips">
        {triggers.map((t) => (
          <span key={t} className="chip">
            {t}
            <button onClick={() => onChange(triggers.filter((x) => x !== t))}>×</button>
          </span>
        ))}
        {triggers.length === 0 && <span className="muted small">Chưa có trigger nào</span>}
      </div>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault()
            add()
          }
        }}
        placeholder="Thêm trigger rồi Enter…"
      />
    </div>
  )
}

function InventoryView({
  inv,
  tab,
  onRemove,
}: {
  inv: Inventory
  tab: Tab
  onRemove: (name: string) => void
}) {
  const [q, setQ] = useState('')
  const ql = q.toLowerCase()

  const filtered = useMemo(() => {
    if (tab === 'skills')
      return inv.skills.filter((s) => (s.name + (s.description ?? '')).toLowerCase().includes(ql))
    if (tab === 'subagents')
      return inv.subagents.filter((s) => (s.name + (s.description ?? '')).toLowerCase().includes(ql))
    return inv.mcpServers.filter((s) => (s.name + (s.description ?? '')).toLowerCase().includes(ql))
  }, [inv, tab, ql])

  return (
    <>
      <input className="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Lọc…" />
      <div className="list">
        {tab === 'skills' &&
          (filtered as Inventory['skills']).map((s) => (
            <div key={s.name} className="item">
              <div className="item-head">
                <span className="name">{s.name}</span>
                <span className="tag">{s.source ?? 'local'}</span>
                {s.source !== 'bundled' && (
                  <button className="del" title="Gỡ" onClick={() => onRemove(s.name)}>🗑</button>
                )}
              </div>
              {s.description && <p className="desc">{s.description}</p>}
              {s.triggers && s.triggers.length > 0 && (
                <div className="mini-chips">
                  {s.triggers.slice(0, 8).map((t) => (
                    <span key={t} className="mini">{t}</span>
                  ))}
                </div>
              )}
            </div>
          ))}

        {tab === 'subagents' &&
          (filtered as Inventory['subagents']).map((s) => (
            <div key={s.name} className="item">
              <div className="item-head">
                <span className="name">🤖 {s.name}</span>
              </div>
              {s.description && <p className="desc">{s.description}</p>}
            </div>
          ))}

        {tab === 'mcp' &&
          (filtered as Inventory['mcpServers']).map((s) => (
            <div key={s.name} className="item">
              <div className="item-head">
                <span className="name">🔌 {s.name}</span>
                {s.transport && <span className="tag">{s.transport}</span>}
              </div>
              {s.description && <p className="desc">{s.description}</p>}
              {s.tools && s.tools.length > 0 && (
                <div className="mini-chips">
                  {s.tools.slice(0, 12).map((t, i) => {
                    const nm = typeof t === 'string' ? t : t.name
                    return (
                      <span key={nm + i} className="mini">{nm}</span>
                    )
                  })}
                </div>
              )}
            </div>
          ))}

        {filtered.length === 0 && <p className="muted">Không có mục nào.</p>}
      </div>
    </>
  )
}
