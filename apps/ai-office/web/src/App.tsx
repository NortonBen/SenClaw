import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from './api'
import { Feed } from './Feed'
import { OfficeScene } from './OfficeScene'
import type { Agent, OfficeEvent, Stats, Task } from './types'

type Panel = 'none' | 'staff' | 'history' | 'ledger' | 'settings'

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [task, setTask] = useState<Task | null>(null)
  const [events, setEvents] = useState<OfficeEvent[]>([])
  const [mode, setMode] = useState<'demo' | 'live'>('demo')
  const [show3d, setShow3d] = useState(true)
  const [panel, setPanel] = useState<Panel>('none')
  const [input, setInput] = useState('')
  const [error, setError] = useState('')
  const [stats, setStats] = useState<Stats | null>(null)
  const [history, setHistory] = useState<Task[]>([])
  const [llmOk, setLlmOk] = useState<boolean | null>(null)

  const taskRef = useRef<number | null>(null)
  const lastEventRef = useRef(0)
  const feedEndRef = useRef<HTMLDivElement>(null)

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
    feedEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [events.length])

  const busy = !!task && !['done', 'error'].includes(task.status)

  const submit = async () => {
    const title = input.trim()
    if (!title || busy) return
    setError('')
    try {
      await api.createTask(title, mode)
      setInput('')
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const openPanel = async (p: Panel) => {
    setPanel(p)
    if (p === 'ledger') setStats(await api.stats().catch(() => null))
    if (p === 'history') setHistory((await api.tasks(50).catch(() => ({ tasks: [] }))).tasks)
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
          className={`btn ${mode}`}
          title="Chế độ cho nhiệm vụ mới: DEMO mô phỏng không gọi API, LIVE chạy LLM thật"
          onClick={() => setMode(mode === 'demo' ? 'live' : 'demo')}
        >
          {mode === 'demo' ? 'DEMO' : '● LIVE'}
        </button>
        <button className="btn" onClick={() => setShow3d(!show3d)}>
          3D: {show3d ? 'BẬT' : 'TẮT'}
        </button>
        <button className="btn" onClick={() => openPanel('ledger')}>Kế toán</button>
        <button className="btn" onClick={() => openPanel('staff')}>Nhân sự</button>
        <button className="btn" onClick={() => openPanel('history')}>Lịch sử</button>
        <button
          className="btn"
          onClick={() => (document.getElementById('task-input') as HTMLInputElement | null)?.focus()}
        >
          Nhiệm vụ mới
        </button>
        <button className="btn" onClick={() => openPanel('settings')}>Cài đặt</button>
      </header>

      <div className="main">
        <aside className="sidebar">
          <div className="cap">Nhân sự trực ca</div>
          {agents.map((a) => (
            <div className="staff" key={a.key}>
              <div className="nm">
                <span
                  className="dot"
                  style={{ background: { working: 'var(--working)', done: 'var(--done)', handoff: 'var(--handoff)' }[a.status] ?? 'var(--idle)' }}
                />
                {a.name}
              </div>
              <div className="rl">{a.role}</div>
              <div className="dt">{a.duty}</div>
              {a.status_note && <div className="st">— {a.status_note}</div>}
            </div>
          ))}
          <div className="hint">
            Gõ nhiệm vụ vào ô bên dưới rồi Enter. Trưởng phòng sẽ phân công agent làm việc &amp; bàn
            giao, cuối cùng nộp báo cáo tổng hợp cho Sếp.
          </div>
        </aside>

        <div className="center">
          {show3d && (
            <div className="scene-wrap">
              <div className="scene-cap">· Mô phỏng văn phòng — trực tiếp</div>
              <OfficeScene agents={agents} events={events} />
              <div className="legend">
                <span><span className="dot" style={{ background: 'var(--working)' }} />đang làm</span>
                <span style={{ color: 'var(--done)' }}>✓ xong</span>
                <span><span className="dot" style={{ background: 'var(--handoff)' }} />đi bàn giao</span>
              </div>
            </div>
          )}
          <Feed events={events} agents={agents} dayLabel={dayLabel} />
          <div ref={feedEndRef} />
          <div className="composer">
            <input
              id="task-input"
              placeholder={busy ? 'Phòng đang xử lý nhiệm vụ…' : 'Giao nhiệm vụ cho phòng, ví dụ: lập kế hoạch marketing ra mắt hệ thống Agent office'}
              value={input}
              disabled={busy}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submit()}
            />
            {busy ? (
              <span className="busy">phòng đang làm việc…</span>
            ) : (
              <button className="btn" onClick={submit}>Giao việc</button>
            )}
          </div>
          {error && <div className="sysline" style={{ padding: '0 16px 8px' }}>⚠ {error}</div>}
        </div>
      </div>

      {panel === 'staff' && (
        <StaffPanel agents={agents} onClose={() => setPanel('none')} onSaved={poll} />
      )}
      {panel === 'history' && <HistoryPanel tasks={history} onClose={() => setPanel('none')} />}
      {panel === 'ledger' && <LedgerPanel stats={stats} onClose={() => setPanel('none')} />}
      {panel === 'settings' && <SettingsPanel llmOk={llmOk} onClose={() => setPanel('none')} />}
    </div>
  )
}

function StaffPanel({
  agents,
  onClose,
  onSaved,
}: {
  agents: Agent[]
  onClose: () => void
  onSaved: () => void
}) {
  const [drafts, setDrafts] = useState<Record<string, { name: string; role: string; duty: string }>>(
    Object.fromEntries(agents.map((a) => [a.key, { name: a.name, role: a.role, duty: a.duty }])),
  )
  const save = async (key: string) => {
    await api.updateAgent(key, drafts[key])
    onSaved()
  }
  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Nhân sự <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <table>
          <thead>
            <tr><th>Tên</th><th>Vai trò</th><th>Nhiệm vụ cố định</th><th /></tr>
          </thead>
          <tbody>
            {agents.map((a) => (
              <tr key={a.key}>
                <td>
                  <input
                    value={drafts[a.key]?.name ?? ''}
                    onChange={(e) => setDrafts({ ...drafts, [a.key]: { ...drafts[a.key], name: e.target.value } })}
                  />
                </td>
                <td>
                  <input
                    value={drafts[a.key]?.role ?? ''}
                    onChange={(e) => setDrafts({ ...drafts, [a.key]: { ...drafts[a.key], role: e.target.value } })}
                  />
                </td>
                <td>
                  <textarea
                    rows={2}
                    value={drafts[a.key]?.duty ?? ''}
                    onChange={(e) => setDrafts({ ...drafts, [a.key]: { ...drafts[a.key], duty: e.target.value } })}
                  />
                </td>
                <td>
                  <button className="row-btn" onClick={() => save(a.key)}>Lưu</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function HistoryPanel({ tasks, onClose }: { tasks: Task[]; onClose: () => void }) {
  const [open, setOpen] = useState<Task | null>(null)
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
            <pre style={{ whiteSpace: 'pre-wrap', fontSize: 12 }}>{open.report || '(chưa có báo cáo)'}</pre>
          </div>
        ) : (
          <table>
            <thead>
              <tr><th>#</th><th>Nhiệm vụ</th><th>Chế độ</th><th>Trạng thái</th><th>LLM</th></tr>
            </thead>
            <tbody>
              {tasks.map((t) => (
                <tr key={t.id}>
                  <td>{t.id}</td>
                  <td><span className="task-title" onClick={() => setOpen(t)}>{t.title}</span></td>
                  <td>{t.mode.toUpperCase()}</td>
                  <td>
                    <span className={`status-pill status-${t.status === 'done' ? 'done' : t.status === 'error' ? 'error' : 'running'}`}>
                      {t.status}
                    </span>
                  </td>
                  <td>{t.llm_calls > 0 ? `${t.llm_calls} calls` : '—'}</td>
                </tr>
              ))}
              {tasks.length === 0 && (
                <tr><td colSpan={5}>Chưa có nhiệm vụ nào — giao việc đầu tiên cho phòng đi Sếp!</td></tr>
              )}
            </tbody>
          </table>
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
            <div className="k">Chạy chế độ LIVE</div><div>{stats.tasksLive}</div>
            <div className="k">Lượt gọi LLM</div><div>{stats.llmCalls}</div>
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

function SettingsPanel({ llmOk, onClose }: { llmOk: boolean | null; onClose: () => void }) {
  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          Cài đặt <button className="btn" onClick={onClose}>Đóng</button>
        </h2>
        <div className="kv">
          <div className="k">Chế độ DEMO</div>
          <div>Mô phỏng toàn bộ quy trình, không gọi API — dùng để xem cách phòng vận hành.</div>
          <div className="k">Chế độ LIVE</div>
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
          <div className="k">Nhân sự</div>
          <div>Đổi tên/vai trò trong mục Nhân sự; personas tương ứng đi kèm app để dùng với Cowork.</div>
        </div>
      </div>
    </div>
  )
}
