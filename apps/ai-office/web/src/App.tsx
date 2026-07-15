import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from './api'
import { Avatar } from './avatar'
import { Feed, Md } from './Feed'
import { OfficeScene } from './OfficeScene'
import { MicButton } from './voice'
import type {
  Agent,
  DirListing,
  KnowledgeSummary,
  OfficeEvent,
  OfficeFeatures,
  OfficeSettings,
  SkillsInventory,
  Stats,
  Task,
  WorkspaceFile,
} from './types'

const FEATURE_ROWS: [keyof OfficeFeatures, string, string][] = [
  ['tools', 'Worker dùng công cụ (MCP / search)', 'Nhân sự có gán skill/sub-agent sẽ chạy như agent thật: gọi được web-search, browser, MCP.'],
  ['memory', 'Trí nhớ riêng mỗi nhân sự', 'Nhớ lại & lưu ký ức vào knowledge space riêng qua mỗi nhiệm vụ.'],
  ['wiki', 'Lưu báo cáo vào wiki', 'Báo cáo tổng hợp tự lưu vào kho wiki của daemon.'],
  ['workspace', 'Đọc / ghi workspace', 'Đọc tài liệu Sếp bỏ vào workspace và ghi kết quả ra file.'],
  ['autocontinue', 'Tự viết tiếp khi bị cắt', 'Nếu LLM cắt giữa chừng, tự yêu cầu viết tiếp cho trọn.'],
]

type Panel = 'none' | 'staff' | 'history' | 'ledger' | 'settings'
type Theme = 'auto' | 'light' | 'dark'

const THEME_LABEL: Record<Theme, string> = {
  auto: '◐ Auto',
  light: '☀ Sáng',
  dark: '🌙 Tối',
}

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [task, setTask] = useState<Task | null>(null)
  const [events, setEvents] = useState<OfficeEvent[]>([])
  const [show3d, setShow3d] = useState(true)
  const [rotation, setRotation] = useState<number>(
    () => Number(localStorage.getItem('ai-office-rot') ?? '0') || 0,
  )
  const dragRef = useRef<{ x: number; rot: number } | null>(null)
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem('ai-office-theme') as Theme) || 'auto',
  )
  const [panel, setPanel] = useState<Panel>('none')
  const [input, setInput] = useState('')
  const [error, setError] = useState('')
  const [stats, setStats] = useState<Stats | null>(null)
  const [history, setHistory] = useState<Task[]>([])
  const [llmOk, setLlmOk] = useState<boolean | null>(null)
  const [queue, setQueue] = useState<Task[]>([])
  const [newTaskOpen, setNewTaskOpen] = useState(false)

  const taskRef = useRef<number | null>(null)
  const lastEventRef = useRef(0)

  const poll = useCallback(async () => {
    try {
      const [{ agents }, { tasks }] = await Promise.all([api.agents(), api.tasks(1)])
      setAgents(agents)
      const latest = tasks[0] ?? null
      setTask(latest)
      if (latest) {
        if (taskRef.current !== latest.id) {
          taskRef.current = latest.id
          lastEventRef.current = 0
          setEvents([])
        }
        const { events: fresh } = await api.events(latest.id, lastEventRef.current)
        if (fresh.length > 0) {
          lastEventRef.current = fresh[fresh.length - 1].id
          setEvents((prev) => [...prev, ...fresh])
        }
      }
      setQueue((await api.queue()).pending)
    } catch {
      /* backend briefly unavailable — next tick will retry */
    }
  }, [])

  useEffect(() => {
    poll()
    const t = setInterval(poll, 1000)
    api.llmInfo().then((i) => setLlmOk(i.available)).catch(() => setLlmOk(false))
    return () => clearInterval(t)
  }, [poll])

  useEffect(() => {
    // 'auto' leaves the choice to prefers-color-scheme (see styles.css).
    if (theme === 'auto') {
      delete document.documentElement.dataset.theme
    } else {
      document.documentElement.dataset.theme = theme
    }
    localStorage.setItem('ai-office-theme', theme)
  }, [theme])

  const cycleTheme = () =>
    setTheme(theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto')

  // Rotation in degrees [0, 360). Persist rounded so refresh is stable.
  const rotate = (deg: number) => {
    const v = ((deg % 360) + 360) % 360
    setRotation(v)
    localStorage.setItem('ai-office-rot', String(Math.round(v)))
  }

  // Drag left/right on the scene to orbit the office freely.
  const onSceneDown = (e: React.PointerEvent) => {
    dragRef.current = { x: e.clientX, rot: rotation }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    ;(e.currentTarget as HTMLElement).classList.add('dragging')
  }
  const onSceneMove = (e: React.PointerEvent) => {
    if (!dragRef.current) return
    rotate(dragRef.current.rot + (e.clientX - dragRef.current.x) * 0.55)
  }
  const onSceneUp = (e: React.PointerEvent) => {
    dragRef.current = null
    ;(e.currentTarget as HTMLElement).classList.remove('dragging')
  }

  const busy = !!task && !['done', 'error'].includes(task.status)

  // Queue a task (works whether busy or idle — the office drains FIFO).
  const addTask = async (title: string) => {
    const t = title.trim()
    if (!t) return
    setError('')
    try {
      await api.createTask(t, 'live')
      poll()
    } catch (e) {
      setError(String((e as Error).message))
      throw e
    }
  }

  const submit = async () => {
    const title = input.trim()
    if (!title) return
    setError('')
    try {
      await api.createTask(title, 'live')
      setInput('')
      poll()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const openPanel = async (p: Panel) => {
    setPanel(p)
    if (p === 'ledger') setStats(await api.stats().catch(() => null))
    if (p === 'history') setHistory((await api.tasks(200).catch(() => ({ tasks: [] }))).tasks)
  }

  const workingCount = agents.filter((a) => a.status === 'working').length
  const dayLabel = `PHÒNG LÀM VIỆC MỞ CỬA — ${new Date().toLocaleDateString('vi-VN')} · ${agents.length} agent trực ca${
    busy ? ` · ${workingCount} đang làm` : ''
  }`

  return (
    <div className="app">
      <header className="hdr">
        <h1>
          AI OFFICE <span>// công ty một người — v1.0</span>
        </h1>
        <div className="spacer" />
        <button
          className="btn"
          title="Giao diện: Auto theo hệ thống / Sáng / Tối"
          onClick={cycleTheme}
        >
          {THEME_LABEL[theme]}
        </button>
        <button className="btn" onClick={() => setShow3d(!show3d)}>
          3D: {show3d ? 'BẬT' : 'TẮT'}
        </button>
        <button className="btn" onClick={() => openPanel('ledger')}>Kế toán</button>
        <button className="btn" onClick={() => openPanel('staff')}>Nhân sự</button>
        <button className="btn" onClick={() => openPanel('history')}>Lịch sử</button>
        <button
          className="btn"
          onClick={() => setNewTaskOpen(true)}
        >
          Nhiệm vụ mới{queue.length > 0 ? ` (${queue.length})` : ''}
        </button>
        <button className="btn" onClick={() => openPanel('settings')}>Cài đặt</button>
      </header>

      <div className="main">
        <aside className="sidebar">
          <div className="cap">Nhân sự trực ca</div>
          {agents.map((a) => (
            <div className="staff" key={a.key} style={a.enabled ? undefined : { opacity: 0.45 }}>
              <div className="nm">
                <Avatar agentKey={a.key} size={20} />
                <span
                  className="dot"
                  style={{ background: a.enabled ? ({ working: 'var(--working)', done: 'var(--done)', handoff: 'var(--handoff)' }[a.status] ?? 'var(--idle)') : 'var(--idle)' }}
                />
                {a.name}
                {!a.enabled && <span style={{ color: 'var(--faint)', fontWeight: 400 }}> (tạm nghỉ)</span>}
              </div>
              <div className="rl">{a.role}</div>
              <div className="dt">{a.duty}</div>
              {a.skills.length > 0 && (
                <div className="dt">⚡ {a.skills.join(', ')}</div>
              )}
              {a.enabled && a.status_note && <div className="st">— {a.status_note}</div>}
            </div>
          ))}
          <div className="hint">
            Gõ nhiệm vụ vào ô bên dưới rồi Enter. Trưởng phòng sẽ phân công agent làm việc &amp; bàn
            giao, cuối cùng nộp báo cáo tổng hợp cho Sếp.
          </div>
        </aside>

        <div className="center">
          {show3d && (
            <div
              className="scene-wrap"
              onPointerDown={onSceneDown}
              onPointerMove={onSceneMove}
              onPointerUp={onSceneUp}
              onPointerCancel={onSceneUp}
            >
              <div className="scene-cap">· Mô phỏng văn phòng — kéo chuột để xoay</div>
              <button
                className="btn scene-rotate"
                title="Xoay nhanh 45°"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => rotate(Math.round(rotation / 45) * 45 + 45)}
              >
                ↻ {Math.round(rotation)}°
              </button>
              <OfficeScene agents={agents} events={events} rotation={rotation} />
              <div className="legend">
                <span><span className="dot" style={{ background: 'var(--working)' }} />đang làm</span>
                <span style={{ color: 'var(--done)' }}>✓ xong</span>
                <span><span className="dot" style={{ background: 'var(--handoff)' }} />đi bàn giao</span>
              </div>
            </div>
          )}
          <Feed events={events} agents={agents} dayLabel={dayLabel} />
          {queue.length > 0 && (
            <div className="queue-bar">
              ⏳ Hàng đợi ({queue.length}):{' '}
              {queue.slice(0, 3).map((q, i) => (
                <span key={q.id}>
                  {i > 0 && ' · '}
                  {q.title.length > 40 ? q.title.slice(0, 39) + '…' : q.title}
                </span>
              ))}
              {queue.length > 3 && ` … +${queue.length - 3}`}
            </div>
          )}
          <div className="composer">
            <MicButton onText={(t) => setInput((prev) => (prev.trim() ? `${prev} ${t}` : t))} />
            <input
              id="task-input"
              placeholder={
                busy
                  ? 'Phòng đang bận — gõ hoặc nói để xếp vào hàng đợi…'
                  : 'Giao nhiệm vụ cho phòng (gõ hoặc bấm 🎤 để nói)…'
              }
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submit()}
            />
            <button className="btn" onClick={submit}>{busy ? '+ Hàng đợi' : 'Giao việc'}</button>
          </div>
          {error && <div className="sysline" style={{ padding: '0 16px 8px' }}>⚠ {error}</div>}
        </div>
      </div>

      {newTaskOpen && (
        <NewTaskDialog
          busy={busy}
          queueLen={queue.length}
          onClose={() => setNewTaskOpen(false)}
          onSubmit={addTask}
        />
      )}
      {panel === 'staff' && (
        <StaffPanel agents={agents} onClose={() => setPanel('none')} onChanged={poll} />
      )}
      {panel === 'history' && <HistoryPanel tasks={history} onClose={() => setPanel('none')} />}
      {panel === 'ledger' && <LedgerPanel stats={stats} onClose={() => setPanel('none')} />}
      {panel === 'settings' && (
        <SettingsPanel
          llmOk={llmOk}
          rotation={rotation}
          onRotate={rotate}
          onClose={() => setPanel('none')}
        />
      )}
    </div>
  )
}

const KIND_LABEL: Record<string, string> = {
  manager: 'Trưởng phòng',
  worker: 'Chuyên môn',
  qa: 'Kiểm định',
}

function StaffPanel({
  agents,
  onClose,
  onChanged,
}: {
  agents: Agent[]
  onClose: () => void
  onChanged: () => void
}) {
  const [editing, setEditing] = useState<Agent | 'new' | null>(null)
  const [detail, setDetail] = useState<Agent | null>(null)
  const [error, setError] = useState('')

  const remove = async (a: Agent) => {
    if (!window.confirm(`Cho ${a.name} nghỉ việc? Bàn làm việc sẽ bị thu hồi.`)) return
    setError('')
    try {
      await api.deleteAgent(a.key)
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const toggleEnabled = async (a: Agent) => {
    setError('')
    try {
      await api.updateAgent(a.key, { enabled: !a.enabled })
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Nhân sự
          <span>
            <button className="btn" onClick={() => setEditing('new')}>+ Tuyển nhân sự</button>{' '}
            <button className="btn" onClick={onClose}>Đóng</button>
          </span>
        </h2>
        {error && <div className="sysline">⚠ {error}</div>}
        <table>
          <thead>
            <tr><th>Tên</th><th>Vai trò</th><th>Loại</th><th>Chế độ</th><th /></tr>
          </thead>
          <tbody>
            {agents.map((a) => (
              <tr key={a.key} style={a.enabled ? undefined : { opacity: 0.5 }}>
                <td>
                  <Avatar agentKey={a.key} size={20} />{' '}
                  <span
                    className="dot"
                    style={{ background: a.enabled ? ({ working: 'var(--working)', done: 'var(--done)', handoff: 'var(--handoff)' }[a.status] ?? 'var(--idle)') : 'var(--idle)' }}
                  />{' '}
                  <b>{a.name}</b>
                  {a.skills.length > 0 && (
                    <div style={{ color: 'var(--faint)', fontSize: 11 }}>⚡ {a.skills.join(', ')}</div>
                  )}
                </td>
                <td>{a.role}</td>
                <td>{KIND_LABEL[a.kind] ?? a.kind}</td>
                <td>
                  {!a.enabled
                    ? 'tạm nghỉ'
                    : a.kind === 'worker'
                      ? a.auto_assign ? 'tự nhận việc' : 'tăng cường'
                      : 'trực ca'}
                </td>
                <td style={{ whiteSpace: 'nowrap' }}>
                  <button className="row-btn" onClick={() => setDetail(a)}>Chi tiết</button>{' '}
                  <button className="row-btn" onClick={() => setEditing(a)}>Sửa</button>{' '}
                  {a.kind !== 'manager' && (
                    <>
                      <button className="row-btn" onClick={() => toggleEnabled(a)}>
                        {a.enabled ? 'Tạm dừng' : 'Kích hoạt'}
                      </button>{' '}
                      <button className="row-btn" style={{ color: 'var(--danger)' }} onClick={() => remove(a)}>
                        Xoá
                      </button>
                    </>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {editing && (
          <StaffDialog
            agent={editing === 'new' ? null : editing}
            agents={agents}
            onClose={() => setEditing(null)}
            onSaved={() => {
              setEditing(null)
              onChanged()
            }}
          />
        )}
        {detail && <StaffDetailDialog agent={detail} onClose={() => setDetail(null)} />}
      </div>
    </div>
  )
}

/** Add / edit one staff member in a focused dialog. */
function StaffDialog({
  agent,
  agents,
  onClose,
  onSaved,
}: {
  agent: Agent | null
  agents: Agent[]
  onClose: () => void
  onSaved: () => void
}) {
  const [name, setName] = useState(agent?.name ?? '')
  const [role, setRole] = useState(agent?.role ?? '')
  const [duty, setDuty] = useState(agent?.duty ?? '')
  const [kind, setKind] = useState(agent?.kind ?? 'worker')
  const [autoAssign, setAutoAssign] = useState(agent?.auto_assign ?? true)
  const [skills, setSkills] = useState<string[]>(agent?.skills ?? [])
  const [inventory, setInventory] = useState<SkillsInventory | null>(null)
  const [skillQuery, setSkillQuery] = useState('')
  const [error, setError] = useState('')
  const hasManager = agents.some((a) => a.kind === 'manager' && a.key !== agent?.key)
  const hasQa = agents.some((a) => a.kind === 'qa' && a.key !== agent?.key)

  useEffect(() => {
    api.skillsInventory().then(setInventory).catch(() => setInventory({ skills: [], personas: [] }))
  }, [])

  const toggleSkill = (n: string) =>
    setSkills((prev) => (prev.includes(n) ? prev.filter((s) => s !== n) : [...prev, n]))

  const save = async () => {
    setError('')
    try {
      if (agent) {
        await api.updateAgent(agent.key, { name, role, duty, auto_assign: autoAssign, skills })
      } else {
        const { agent: created } = await api.addAgent({ name, role, duty, kind })
        if (!autoAssign || skills.length > 0) {
          await api.updateAgent(created.key, { auto_assign: autoAssign, skills })
        }
      }
      onSaved()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(520px, 90vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          {agent ? `Sửa hồ sơ — ${agent.name}` : 'Tuyển nhân sự mới'}
          <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <div className="form-grid">
          <label>Tên hiển thị</label>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="VD: THIẾT KẾ" />
          <label>Vai trò</label>
          <input value={role} onChange={(e) => setRole(e.target.value)} placeholder="VD: Thiết kế & hình ảnh" />
          <label>Nhiệm vụ cố định</label>
          <textarea
            rows={4}
            value={duty}
            onChange={(e) => setDuty(e.target.value)}
            placeholder="Mô tả nhiệm vụ mà nhân sự này luôn đảm nhận trong quy trình…"
          />
          <label>Loại</label>
          {agent ? (
            <div>{KIND_LABEL[agent.kind] ?? agent.kind} (không đổi được)</div>
          ) : (
            <select value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="worker">Chuyên môn (worker)</option>
              <option value="manager" disabled={hasManager}>Trưởng phòng {hasManager ? '— đã có' : ''}</option>
              <option value="qa" disabled={hasQa}>Kiểm định {hasQa ? '— đã có' : ''}</option>
            </select>
          )}
          {(agent?.kind ?? kind) === 'worker' && (
            <>
              <label>Nhận việc</label>
              <label style={{ textTransform: 'none', letterSpacing: 0, fontSize: 12, color: 'var(--ink)', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={autoAssign}
                  onChange={(e) => setAutoAssign(e.target.checked)}
                />{' '}
                Tự nhận nhiệm vụ — luôn có phần việc trong mọi kế hoạch. Bỏ chọn = tăng cường
                (Trưởng phòng chỉ giao khi cần chuyên môn này).
              </label>
            </>
          )}
          <label>Skill / sub-agent nắm giữ</label>
          <div>
            {skills.length > 0 && (
              <div className="chips">
                {skills.map((s) => (
                  <span className="chip" key={s}>
                    ⚡ {s}
                    <button type="button" onClick={() => toggleSkill(s)} title="Bỏ chọn">×</button>
                  </span>
                ))}
              </div>
            )}
            <input
              placeholder="🔍 Tìm skill / sub-agent…"
              value={skillQuery}
              onChange={(e) => setSkillQuery(e.target.value)}
              style={{ marginBottom: 6 }}
            />
            <div className="skill-picker">
              {inventory === null && <div style={{ color: 'var(--faint)' }}>Đang tải danh mục…</div>}
              {inventory && inventory.skills.length === 0 && inventory.personas.length === 0 && (
                <div style={{ color: 'var(--faint)' }}>
                  Không lấy được danh mục từ daemon — kiểm tra SenClaw daemon.
                </div>
              )}
              {(['skills', 'personas'] as const).map((group) => {
                if (!inventory) return null
                const q = skillQuery.trim().toLowerCase()
                const items = inventory[group].filter(
                  (it) =>
                    !q ||
                    it.name.toLowerCase().includes(q) ||
                    it.description.toLowerCase().includes(q),
                )
                if (items.length === 0) return null
                return (
                  <div key={group}>
                    <div className="cap">{group === 'skills' ? 'Skills' : 'Sub-agents'} ({items.length})</div>
                    {items.map((it) => (
                      <label key={it.name} title={it.description}>
                        <input
                          type="checkbox"
                          checked={skills.includes(it.name)}
                          onChange={() => toggleSkill(it.name)}
                        />{' '}
                        {it.name}
                        {it.description && (
                          <span className="skill-desc"> — {it.description.slice(0, 70)}{it.description.length > 70 ? '…' : ''}</span>
                        )}
                      </label>
                    ))}
                  </div>
                )
              })}
              {inventory &&
                skillQuery.trim() !== '' &&
                !inventory.skills.some((s) => s.name.toLowerCase().includes(skillQuery.trim().toLowerCase()) || s.description.toLowerCase().includes(skillQuery.trim().toLowerCase())) &&
                !inventory.personas.some((p) => p.name.toLowerCase().includes(skillQuery.trim().toLowerCase()) || p.description.toLowerCase().includes(skillQuery.trim().toLowerCase())) && (
                  <div style={{ color: 'var(--faint)' }}>Không có mục nào khớp "{skillQuery}".</div>
                )}
            </div>
          </div>
        </div>
        {error && <div className="sysline">⚠ {error}</div>}
        <div style={{ marginTop: 12, textAlign: 'right' }}>
          <button className="btn" onClick={save} disabled={!name.trim()}>
            {agent ? 'Lưu hồ sơ' : 'Tuyển vào phòng'}
          </button>
        </div>
      </div>
    </div>
  )
}

/** Read-only detail: profile summary only. The private memory shows just a
 *  count — browsing the actual items belongs to the Knowledge screen
 *  (desktop_app) with its space picker. */
function StaffDetailDialog({ agent, onClose }: { agent: Agent; onClose: () => void }) {
  const [mem, setMem] = useState<KnowledgeSummary | null>(null)
  const [memErr, setMemErr] = useState('')

  useEffect(() => {
    api
      .agentKnowledge(agent.key)
      .then(setMem)
      .catch((e) => setMemErr(String((e as Error).message)))
  }, [agent.key])

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(560px, 90vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
            <Avatar agentKey={agent.key} size={26} />
            {agent.name}
          </span>
          <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <div className="kv">
          <div className="k">Loại</div><div>{KIND_LABEL[agent.kind] ?? agent.kind}</div>
          <div className="k">Vai trò</div><div>{agent.role || '—'}</div>
          <div className="k">Nhiệm vụ cố định</div><div><Md>{agent.duty || '—'}</Md></div>
          <div className="k">Chế độ</div>
          <div>
            {!agent.enabled
              ? 'Tạm nghỉ — không tham gia nhiệm vụ'
              : agent.kind === 'worker'
                ? agent.auto_assign
                  ? 'Tự nhận nhiệm vụ — luôn có phần việc'
                  : 'Tăng cường — chỉ được giao khi cần chuyên môn'
                : 'Trực ca'}
          </div>
          <div className="k">Skill / sub-agent</div>
          <div>{agent.skills.length > 0 ? agent.skills.map((s) => `⚡ ${s}`).join('  ') : '—'}</div>
          <div className="k">Trạng thái</div><div>{agent.status}{agent.status_note ? ` — ${agent.status_note}` : ''}</div>
          <div className="k">Trí nhớ riêng</div>
          <div>
            {memErr && <span style={{ color: 'var(--faint)' }}>không đọc được ({memErr})</span>}
            {!memErr && mem === null && 'đang đếm…'}
            {!memErr && mem !== null && (
              <>
                {mem.count >= 100 ? '100+' : mem.count} ký ức trong space <code>{mem.space}</code>
                <div style={{ color: 'var(--faint)', fontSize: 11 }}>
                  Xem chi tiết trong Knowledge (desktop app) — chọn space này ở bộ lọc.
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

/** Quick "new task" dialog — always queues (works while the office is busy). */
function NewTaskDialog({
  busy,
  queueLen,
  onClose,
  onSubmit,
}: {
  busy: boolean
  queueLen: number
  onClose: () => void
  onSubmit: (title: string) => Promise<void>
}) {
  const [text, setText] = useState('')
  const [err, setErr] = useState('')
  const submit = async () => {
    if (!text.trim()) return
    try {
      await onSubmit(text)
      onClose()
    } catch (e) {
      setErr(String((e as Error).message))
    }
  }
  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(560px, 92vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          Nhiệm vụ mới <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <div style={{ color: 'var(--faint)', fontSize: 12, marginBottom: 8 }}>
          {busy
            ? `Phòng đang bận — nhiệm vụ này sẽ xếp vào hàng đợi (hiện có ${queueLen} chờ) và tự chạy khi xong việc trước.`
            : 'Trưởng phòng sẽ nhận và phân công cho cả phòng ngay.'}
        </div>
        <textarea
          rows={4}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => (e.key === 'Enter' && (e.metaKey || e.ctrlKey) ? submit() : undefined)}
          placeholder="Ví dụ: nghiên cứu 5 xu hướng nội thất 2026 và đề xuất bộ sưu tập ra mắt"
          style={{ width: '100%', border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: 8 }}
        />
        {err && <div className="sysline">⚠ {err}</div>}
        <div style={{ marginTop: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <MicButton onText={(t) => setText((prev) => (prev.trim() ? `${prev} ${t}` : t))} />
          <button className="btn" onClick={submit} disabled={!text.trim()}>
            {busy ? '+ Xếp hàng đợi' : 'Giao việc'}
          </button>
        </div>
      </div>
    </div>
  )
}

const HISTORY_PER_PAGE = 15

function HistoryPanel({ tasks, onClose }: { tasks: Task[]; onClose: () => void }) {
  const [open, setOpen] = useState<Task | null>(null)
  const [page, setPage] = useState(0)
  const pages = Math.max(1, Math.ceil(tasks.length / HISTORY_PER_PAGE))
  const p = Math.min(page, pages - 1)
  const slice = tasks.slice(p * HISTORY_PER_PAGE, p * HISTORY_PER_PAGE + HISTORY_PER_PAGE)
  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Lịch sử nhiệm vụ <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        {open ? (
          <div>
            <button className="row-btn" onClick={() => setOpen(null)}>← danh sách</button>
            <h3 style={{ fontSize: 13 }}>{open.title}</h3>
            <Md>{open.report || '*(chưa có báo cáo)*'}</Md>
          </div>
        ) : (
          <>
            <table>
              <thead>
                <tr><th>#</th><th>Nhiệm vụ</th><th>Trạng thái</th><th>LLM</th><th>Token</th></tr>
              </thead>
              <tbody>
                {slice.map((t) => (
                  <tr key={t.id}>
                    <td>{t.id}</td>
                    <td><span className="task-title" onClick={() => setOpen(t)}>{t.title}</span></td>
                    <td>
                      <span className={`status-pill status-${t.status === 'done' ? 'done' : t.status === 'error' ? 'error' : 'running'}`}>
                        {t.status}
                      </span>
                    </td>
                    <td>{t.llm_calls > 0 ? `${t.llm_calls} calls` : '—'}</td>
                    <td>{t.tokens_in + t.tokens_out > 0 ? `~${(t.tokens_in + t.tokens_out).toLocaleString('vi-VN')}` : '—'}</td>
                  </tr>
                ))}
                {tasks.length === 0 && (
                  <tr><td colSpan={5}>Chưa có nhiệm vụ nào — giao việc đầu tiên cho phòng đi Sếp!</td></tr>
                )}
              </tbody>
            </table>
            {pages > 1 && (
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'center', marginTop: 10 }}>
                <button className="btn" disabled={p === 0} onClick={() => setPage(p - 1)}>← Trước</button>
                <span style={{ fontSize: 12 }}>Trang {p + 1}/{pages} · {tasks.length} nhiệm vụ</span>
                <button className="btn" disabled={p >= pages - 1} onClick={() => setPage(p + 1)}>Sau →</button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

function LedgerPanel({ stats, onClose }: { stats: Stats | null; onClose: () => void }) {
  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Kế toán <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        {stats ? (
          <div className="kv">
            <div className="k">Tổng nhiệm vụ</div><div>{stats.tasksTotal}</div>
            <div className="k">Đã hoàn thành</div><div>{stats.tasksDone}</div>
            <div className="k">Lượt gọi LLM</div><div>{stats.llmCalls}</div>
            <div className="k">Token đã dùng (ước tính)</div>
            <div>
              {stats.tokensIn + stats.tokensOut > 0
                ? `${(stats.tokensIn + stats.tokensOut).toLocaleString('vi-VN')} (vào ${stats.tokensIn.toLocaleString('vi-VN')} · ra ${stats.tokensOut.toLocaleString('vi-VN')})`
                : '0'}
            </div>
            <div className="k">Model gần nhất</div><div>{stats.lastModel || '—'}</div>
            <div className="k">Lương nhân sự</div><div>0 ₫ (agent không nhận lương 😜)</div>
          </div>
        ) : (
          <div>Đang tải…</div>
        )}
      </div>
    </div>
  )
}

function SettingsPanel({
  llmOk,
  rotation,
  onRotate,
  onClose,
}: {
  llmOk: boolean | null
  rotation: number
  onRotate: (r: number) => void
  onClose: () => void
}) {
  const [settings, setSettings] = useState<OfficeSettings | null>(null)
  const [wsDir, setWsDir] = useState('')
  const [wsFiles, setWsFiles] = useState<WorkspaceFile[]>([])
  const [wsMsg, setWsMsg] = useState('')
  const [pickerOpen, setPickerOpen] = useState(false)

  const loadWs = async () => {
    try {
      const s = await api.settings()
      setSettings(s)
      setWsDir(s.workspaceDir)
      setWsFiles((await api.workspaceFiles()).files.slice(0, 8))
    } catch {
      /* daemon/app briefly unavailable */
    }
  }

  useEffect(() => {
    loadWs()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const saveWs = async () => {
    setWsMsg('')
    try {
      const s = await api.updateSettings({ workspaceDir: wsDir.trim() })
      setSettings(s)
      setWsDir(s.workspaceDir)
      setWsMsg('✓ đã lưu')
      setWsFiles((await api.workspaceFiles()).files.slice(0, 8))
    } catch (e) {
      setWsMsg(`⚠ ${String((e as Error).message)}`)
    }
  }

  const toggleFeature = async (key: keyof OfficeFeatures, val: boolean) => {
    setSettings((prev) => (prev ? { ...prev, features: { ...prev.features, [key]: val } } : prev))
    try {
      setSettings(await api.updateSettings({ features: { [key]: val } }))
    } catch {
      /* revert on next settings load */
    }
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Cài đặt <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <div className="kv">
          <div className="k">Workspace folder</div>
          <div>
            <div style={{ display: 'flex', gap: 6 }}>
              <input
                style={{ flex: 1, border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: '4px 6px' }}
                value={wsDir}
                onChange={(e) => setWsDir(e.target.value)}
                placeholder="~/Documents/ai-office hoặc đường dẫn tuyệt đối"
              />
              <button className="btn" onClick={() => setPickerOpen(true)}>Chọn…</button>
              <button className="btn" onClick={saveWs}>Lưu</button>
            </div>
            <div style={{ color: 'var(--faint)', fontSize: 11, marginTop: 4 }}>
              Kho tài liệu chung của phòng: Sếp bỏ tệp tham khảo vào đây (mở bằng Finder),
              nhân sự sẽ đọc khi làm việc và ghi kết quả vào <code>task-&lt;id&gt;/…</code>.
              Để trống rồi Lưu = quay về thư mục mặc định.
              {settings && (
                <> Hiện có <b>{settings.workspaceFiles}</b> tệp{settings.workspaceIsDefault ? ' (mặc định)' : ''}.</>
              )}
            </div>
            {wsMsg && <div style={{ fontSize: 11, marginTop: 2 }}>{wsMsg}</div>}
            {wsFiles.length > 0 && (
              <div style={{ marginTop: 6, fontSize: 11, color: 'var(--faint)' }}>
                {wsFiles.map((f) => (
                  <div key={f.rel}>📄 {f.rel} · {(f.size / 1024).toFixed(1)} KB</div>
                ))}
              </div>
            )}
          </div>
          <div className="k">Góc nhìn văn phòng</div>
          <div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                type="range"
                min={0}
                max={359}
                value={Math.round(rotation)}
                onChange={(e) => onRotate(Number(e.target.value))}
                style={{ flex: 1 }}
              />
              <span style={{ minWidth: 40, textAlign: 'right' }}>{Math.round(rotation)}°</span>
            </div>
            <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 6 }}>
              {[0, 45, 90, 135, 180, 225, 270, 315].map((deg) => (
                <button
                  key={deg}
                  className="btn"
                  style={Math.round(rotation) === deg ? { background: 'var(--ink)', color: 'var(--paper)' } : undefined}
                  onClick={() => onRotate(deg)}
                >
                  {deg}°
                </button>
              ))}
            </div>
            <div style={{ color: 'var(--faint)', fontSize: 11, marginTop: 4 }}>
              Xoay tự do 360° quanh tâm sàn — kéo chuột trái/phải ngay trên khung mô phỏng,
              hoặc chỉnh bằng thanh trượt / nút góc ở đây.
            </div>
          </div>
          <div className="k feat-head">Chức năng phòng</div>
          <div className="feat-list">
            {FEATURE_ROWS.map(([key, label, desc]) => {
              const on = settings?.features?.[key] ?? true
              return (
                <label key={key} className="feat-row">
                  <span className="switch">
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={(e) => toggleFeature(key, e.target.checked)}
                    />
                    <span className="slider" />
                  </span>
                  <span className="feat-text">
                    <b>{label}</b>
                    <span className="feat-desc">{desc}</span>
                  </span>
                </label>
              )
            })}
          </div>
          <div className="k">Vận hành</div>
          <div>
            Mỗi agent xử lý thật phần việc của mình qua LLM của SenClaw daemon.
            {llmOk === false && <span style={{ color: 'var(--danger)' }}> Hiện không kết nối được daemon LLM.</span>}
            {llmOk === true && <span style={{ color: 'var(--done)' }}> Daemon LLM sẵn sàng.</span>}
          </div>
          <div className="k">MCP cho agent ngoài</div>
          <div>
            Server <code>ai-office-mcp</code> — agent SenClaw có thể giao việc bằng{' '}
            <code>office_create_task</code> và lấy kết quả bằng <code>office_get_report</code>.
          </div>
        </div>
        {pickerOpen && (
          <FolderPicker
            initial={wsDir}
            onClose={() => setPickerOpen(false)}
            onSelect={async (path) => {
              setPickerOpen(false)
              setWsDir(path)
              setWsMsg('')
              try {
                const s = await api.updateSettings({ workspaceDir: path })
                setSettings(s)
                setWsDir(s.workspaceDir)
                setWsMsg('✓ đã lưu')
                setWsFiles((await api.workspaceFiles()).files.slice(0, 8))
              } catch (e) {
                setWsMsg(`⚠ ${String((e as Error).message)}`)
              }
            }}
          />
        )}
      </div>
    </div>
  )
}

/** Server-side folder browser: the iframe has no native folder dialog, so we
 *  walk directories through the app's own /api/fs/dirs. */
function FolderPicker({
  initial,
  onClose,
  onSelect,
}: {
  initial: string
  onClose: () => void
  onSelect: (path: string) => void
}) {
  const [listing, setListing] = useState<DirListing | null>(null)
  const [error, setError] = useState('')

  const load = async (path?: string) => {
    setError('')
    try {
      setListing(await api.fsDirs(path))
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  useEffect(() => {
    load(initial || undefined)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const shortcuts: [string, string][] = listing
    ? [
        ['🏠 Home', listing.home],
        ['📄 Documents', `${listing.home}/Documents`],
        ['🖥 Desktop', `${listing.home}/Desktop`],
        ['⬇ Downloads', `${listing.home}/Downloads`],
      ]
    : []

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(560px, 90vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          Chọn workspace folder
          <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        {shortcuts.length > 0 && (
          <div className="chips" style={{ marginBottom: 8 }}>
            {shortcuts.map(([label, path]) => (
              <span className="chip" key={label} style={{ cursor: 'pointer' }} onClick={() => load(path)}>
                {label}
              </span>
            ))}
          </div>
        )}
        <div style={{ fontSize: 12, marginBottom: 6 }}>
          📁 <code>{listing?.path ?? '…'}</code>
        </div>
        {error && <div className="sysline">⚠ {error}</div>}
        <div className="dir-list">
          {listing?.parent && (
            <div className="dir-row" onClick={() => load(listing.parent!)}>⬆ .. (lên thư mục cha)</div>
          )}
          {listing?.dirs.map((d) => (
            <div className="dir-row" key={d} onClick={() => load(`${listing.path}/${d}`)}>
              📁 {d}
            </div>
          ))}
          {listing && listing.dirs.length === 0 && (
            <div style={{ color: 'var(--faint)', padding: 6 }}>(không có thư mục con)</div>
          )}
        </div>
        <div style={{ marginTop: 10, textAlign: 'right' }}>
          <button className="btn" disabled={!listing} onClick={() => listing && onSelect(listing.path)}>
            ✓ Chọn thư mục này
          </button>
        </div>
      </div>
    </div>
  )
}
