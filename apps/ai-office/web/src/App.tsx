import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from './api'
import { Avatar } from './avatar'
import { BoardView } from './Board'
import { DashboardView } from './Dashboard'
import { Feed, Md } from './Feed'
import { OfficeScene } from './OfficeScene'
import { MicButton } from './voice'
import { setLang, tr, getLang, type Lang } from './i18n'
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
  Team,
  WorkspaceFile,
} from './types'

const FEATURE_ROWS: [keyof OfficeFeatures, string, string][] = [
  ['tools', 'Worker dùng công cụ (MCP / search)', 'Nhân sự có gán skill/sub-agent sẽ chạy như agent thật: gọi được web-search, browser, MCP.'],
  ['memory', 'Trí nhớ riêng mỗi nhân sự', 'Nhớ lại & lưu ký ức vào knowledge space riêng qua mỗi nhiệm vụ.'],
  ['wiki', 'Lưu báo cáo vào wiki', 'Báo cáo tổng hợp tự lưu vào kho wiki của daemon.'],
  ['workspace', 'Đọc / ghi workspace', 'Đọc tài liệu Sếp bỏ vào workspace và ghi kết quả ra file.'],
  ['autocontinue', 'Tự viết tiếp khi bị cắt', 'Nếu LLM cắt giữa chừng, tự yêu cầu viết tiếp cho trọn.'],
]

type Panel = 'none' | 'ledger' | 'settings'
type Theme = 'auto' | 'light' | 'dark'
/** Các mặt của trụ sở (tab như OPC HQ): bàn Sếp (dashboard), bảng việc
 *  (kanban), sàn văn phòng, sơ đồ nhân sự, nhật ký nhiệm vụ. */
type View = 'hq' | 'board' | 'office' | 'staff' | 'history'

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
  const [zoom, setZoom] = useState<number>(
    () => Number(localStorage.getItem('ai-office-zoom') ?? '1') || 1,
  )
  const [pan, setPan] = useState<{ fx: number; fy: number }>({ fx: 0, fy: 0 })
  const sceneRef = useRef<HTMLDivElement | null>(null)
  const dragRef = useRef<
    | { mode: 'rotate'; x: number; rot: number }
    | { mode: 'pan'; x: number; y: number; fx: number; fy: number }
    | null
  >(null)
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem('ai-office-theme') as Theme) || 'auto',
  )
  const [lang, setLangState] = useState<Lang>(
    () => (localStorage.getItem('ai-office-lang') as Lang) || 'vi',
  )
  // Apply before children render so every tr() this pass sees the language.
  setLang(lang)
  const [panel, setPanel] = useState<Panel>('none')
  const [input, setInput] = useState('')
  const [error, setError] = useState('')
  const [stats, setStats] = useState<Stats | null>(null)
  const [llmOk, setLlmOk] = useState<boolean | null>(null)
  const [queue, setQueue] = useState<Task[]>([])
  const [teams, setTeams] = useState<Team[]>([])
  const [activeTeam, setActiveTeam] = useState<string>(
    () => localStorage.getItem('ai-office-team') ?? '',
  )
  const [view, setViewState] = useState<View>(() => {
    const v = localStorage.getItem('ai-office-view')
    return v === 'board' || v === 'office' || v === 'staff' || v === 'history' ? v : 'hq'
  })
  const setView = (v: View) => {
    setViewState(v)
    localStorage.setItem('ai-office-view', v)
  }

  const taskRef = useRef<number | null>(null)
  const lastEventRef = useRef(0)
  const activeTeamRef = useRef(activeTeam)
  activeTeamRef.current = activeTeam

  const poll = useCallback(async () => {
    try {
      const [{ teams }, { agents }] = await Promise.all([api.teams(), api.agents()])
      setTeams(teams)
      setAgents(agents)
      // Pick a valid active team (default to first).
      let team = activeTeamRef.current
      if (!team || !teams.some((t) => t.key === team)) {
        team = teams[0]?.key ?? ''
        if (team) {
          activeTeamRef.current = team
          setActiveTeam(team)
        }
      }
      const { tasks } = await api.tasks(1, team)
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
      } else {
        taskRef.current = null
        setEvents([])
      }
      setQueue((await api.queue()).pending.filter((t) => t.team === team))
    } catch {
      /* backend briefly unavailable — next tick will retry */
    }
  }, [])

  // Switching teams resets the feed to that team's current task.
  const selectTeam = (key: string) => {
    setActiveTeam(key)
    localStorage.setItem('ai-office-team', key)
    taskRef.current = null
    lastEventRef.current = 0
    setEvents([])
    setTask(null)
    poll()
  }

  const teamAgents = agents.filter((a) => a.team === activeTeam)

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

  const setLangPersist = (next: Lang) => {
    setLangState(next)
    localStorage.setItem('ai-office-lang', next)
  }

  // Rotation in degrees [0, 360). Persist rounded so refresh is stable.
  const rotate = (deg: number) => {
    const v = ((deg % 360) + 360) % 360
    setRotation(v)
    localStorage.setItem('ai-office-rot', String(Math.round(v)))
  }

  const setZoomClamped = (z: number) => {
    const v = Math.min(4, Math.max(0.4, z))
    setZoom(v)
    localStorage.setItem('ai-office-zoom', String(v.toFixed(2)))
  }
  const resetView = () => {
    setZoomClamped(1)
    setPan({ fx: 0, fy: 0 })
  }

  // Left-drag orbits the office; Shift-drag (or middle/right button) pans the
  // view; the mouse wheel zooms. Reset with the ⤢ button.
  const onSceneDown = (e: React.PointerEvent) => {
    const panMode = e.shiftKey || e.button === 1 || e.button === 2
    if (panMode) {
      dragRef.current = { mode: 'pan', x: e.clientX, y: e.clientY, fx: pan.fx, fy: pan.fy }
    } else {
      dragRef.current = { mode: 'rotate', x: e.clientX, rot: rotation }
    }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    ;(e.currentTarget as HTMLElement).classList.add('dragging')
  }
  const onSceneMove = (e: React.PointerEvent) => {
    const d = dragRef.current
    if (!d) return
    if (d.mode === 'rotate') {
      rotate(d.rot + (e.clientX - d.x) * 0.55)
    } else {
      const rect = sceneRef.current?.getBoundingClientRect()
      const w = rect?.width || 1
      const h = rect?.height || 1
      setPan({ fx: d.fx + (e.clientX - d.x) / w, fy: d.fy + (e.clientY - d.y) / h })
    }
  }
  const onSceneUp = (e: React.PointerEvent) => {
    dragRef.current = null
    ;(e.currentTarget as HTMLElement).classList.remove('dragging')
  }

  // Wheel-to-zoom (native listener so we can preventDefault the page scroll).
  useEffect(() => {
    const el = sceneRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      setZoomClamped(zoom * (e.deltaY < 0 ? 1.12 : 1 / 1.12))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [zoom])

  const busy = !!task && !['done', 'error'].includes(task.status)

  const submit = async () => {
    const title = input.trim()
    if (!title || !activeTeam) return
    setError('')
    try {
      await api.createTask(title, activeTeam)
      setInput('')
      poll()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const openPanel = async (p: Panel) => {
    setPanel(p)
    if (p === 'ledger') setStats(await api.stats().catch(() => null))
  }

  const workingCount = teamAgents.filter((a) => a.status === 'working').length
  const teamName = tr(teams.find((t) => t.key === activeTeam)?.name ?? 'ĐỘI')
  const dayLabel = `${teamName} — ${new Date().toLocaleDateString(getLang() === 'en' ? 'en-US' : 'vi-VN')} · ${teamAgents.length} ${tr('agent trực ca')}${
    busy ? ` · ${workingCount} ${tr('đang làm')}` : ''
  }`

  return (
    <div className="app">
      <header className="hdr">
        <h1>
          AI OFFICE <span>// {tr('công ty một người — v1.0')}</span>
        </h1>
        <div className="spacer" />
        <button
          className="btn"
          title={tr('Giao diện: Auto theo hệ thống / Sáng / Tối')}
          onClick={cycleTheme}
        >
          {tr(THEME_LABEL[theme])}
        </button>
        <button className="btn" onClick={() => setShow3d(!show3d)}>
          3D: {show3d ? tr('BẬT') : tr('TẮT')}
        </button>
        <button className="btn" onClick={() => openPanel('ledger')}>{tr('Kế toán')}</button>
        <button className="btn" onClick={() => openPanel('settings')}>{tr('Cài đặt')}</button>
      </header>

      <div className="view-tabs">
        {(
          [
            ['hq', `☀ ${tr('ĐIỀU HÀNH')}`],
            ['board', `📋 ${tr('BẢNG VIỆC')}`],
            ['office', `🏢 ${tr('VĂN PHÒNG')}`],
            ['staff', `👥 ${tr('NHÂN SỰ')}`],
            ['history', `📜 ${tr('LỊCH SỬ')}`],
          ] as [View, string][]
        ).map(([v, label]) => (
          <button
            key={v}
            className={`view-tab${view === v ? ' active' : ''}`}
            onClick={() => setView(v)}
          >
            {v === 'office' &&
            agents.some((a) => a.enabled && (a.status === 'working' || a.status === 'handoff')) ? (
              <span className="team-dot" />
            ) : null}
            {label}
          </button>
        ))}
      </div>

      {view === 'hq' && <DashboardView onOpenBoard={() => setView('board')} />}
      {view === 'board' && <BoardView teams={teams} />}
      {view === 'staff' && (
        <StaffView
          agents={agents}
          teams={teams}
          activeTeam={activeTeam}
          onSelectTeam={selectTeam}
          onChanged={poll}
        />
      )}
      {view === 'history' && <HistoryView />}

      {view === 'office' && (
        <div className="team-tabs">
          {teams.map((t) => {
            const running = agents.some(
              (a) => a.team === t.key && a.enabled && (a.status === 'working' || a.status === 'handoff'),
            )
            return (
              <button
                key={t.key}
                className={`team-tab${t.key === activeTeam ? ' active' : ''}`}
                title={tr(t.description)}
                onClick={() => selectTeam(t.key)}
              >
                {running && <span className="team-dot" />}
                {tr(t.name)}
              </button>
            )
          })}
          <button className="team-tab add" title={tr('Quản lý đội nhóm')} onClick={() => setView('staff')}>
            + {tr('Đội')}
          </button>
        </div>
      )}

      {view === 'office' && (
      <div className="main">
        <aside className="sidebar">
          <div className="cap">{tr('Nhân sự')} · {teamName}</div>
          {teamAgents.map((a) => (
            <div className="staff" key={a.key} style={a.enabled ? undefined : { opacity: 0.45 }}>
              <div className="nm">
                <Avatar agentKey={a.key} size={20} />
                <span
                  className="dot"
                  style={{ background: a.enabled ? ({ working: 'var(--working)', done: 'var(--done)', handoff: 'var(--handoff)' }[a.status] ?? 'var(--idle)') : 'var(--idle)' }}
                />
                {tr(a.name)}
                {!a.enabled && <span style={{ color: 'var(--faint)', fontWeight: 400 }}> ({tr('tạm nghỉ')})</span>}
              </div>
              <div className="rl">{tr(a.role)}</div>
              <div className="dt">{tr(a.duty)}</div>
              {a.skills.length > 0 && (
                <div className="dt">⚡ {a.skills.join(', ')}</div>
              )}
              {a.enabled && a.status_note && <div className="st">— {a.status_note}</div>}
            </div>
          ))}
          <div className="hint">
            {tr('Gõ nhiệm vụ vào ô bên dưới rồi Enter. Trưởng phòng sẽ phân công agent làm việc & bàn giao, cuối cùng nộp báo cáo tổng hợp cho Sếp.')}
          </div>
        </aside>

        <div className="center">
          {show3d && (
            <div
              className="scene-wrap"
              ref={sceneRef}
              onPointerDown={onSceneDown}
              onPointerMove={onSceneMove}
              onPointerUp={onSceneUp}
              onPointerCancel={onSceneUp}
              onContextMenu={(e) => e.preventDefault()}
            >
              <div className="scene-cap">{tr('· Mô phỏng — kéo để xoay · lăn chuột phóng to · giữ Shift kéo để dời')}</div>
              <div className="scene-tools" onPointerDown={(e) => e.stopPropagation()}>
                <button className="btn scene-rotate" title={tr('Xoay nhanh 45°')} onClick={() => rotate(Math.round(rotation / 45) * 45 + 45)}>
                  ↻ {Math.round(rotation)}°
                </button>
                <button className="btn scene-zbtn" title={tr('Phóng to')} onClick={() => setZoomClamped(zoom * 1.2)}>+</button>
                <button className="btn scene-zbtn" title={tr('Thu nhỏ')} onClick={() => setZoomClamped(zoom / 1.2)}>−</button>
                <button className="btn scene-zbtn" title={tr('Về mặc định')} onClick={resetView}>⤢</button>
              </div>
              <OfficeScene
                agents={agents}
                teams={teams}
                activeTeam={activeTeam}
                events={events}
                rotation={rotation}
                zoom={zoom}
                pan={pan}
              />
              <div className="legend">
                <span><span className="dot" style={{ background: 'var(--working)' }} />{tr('đang làm')}</span>
                <span style={{ color: 'var(--done)' }}>✓ {tr('xong')}</span>
                <span><span className="dot" style={{ background: 'var(--handoff)' }} />{tr('đi bàn giao')}</span>
              </div>
            </div>
          )}
          <Feed events={events} agents={agents} dayLabel={dayLabel} />
          {queue.length > 0 && (
            <div className="queue-bar">
              ⏳ {tr('Hàng đợi')} ({queue.length}):{' '}
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
                  ? tr('Phòng đang bận — gõ hoặc nói để xếp vào hàng đợi…')
                  : tr('Giao nhiệm vụ cho phòng (gõ hoặc bấm 🎤 để nói)…')
              }
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submit()}
            />
            <button className="btn" onClick={submit}>{busy ? `+ ${tr('Hàng đợi')}` : tr('Giao việc')}</button>
          </div>
          {error && <div className="sysline" style={{ padding: '0 16px 8px' }}>⚠ {error}</div>}
        </div>
      </div>
      )}

      {panel === 'ledger' && <LedgerPanel stats={stats} onClose={() => setPanel('none')} />}
      {panel === 'settings' && (
        <SettingsPanel
          llmOk={llmOk}
          rotation={rotation}
          onRotate={rotate}
          lang={lang}
          onSetLang={setLangPersist}
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

/** Tab NHÂN SỰ — sơ đồ đội nhóm & biên chế, trang inline như OPC HQ
 *  (không còn là modal; các dialog thêm/sửa vẫn overlay bên trong). */
function StaffView({
  agents,
  teams,
  activeTeam,
  onSelectTeam,
  onChanged,
}: {
  agents: Agent[]
  teams: Team[]
  activeTeam: string
  onSelectTeam: (key: string) => void
  onChanged: () => void
}) {
  const [editing, setEditing] = useState<Agent | 'new' | null>(null)
  const [detail, setDetail] = useState<Agent | null>(null)
  const [error, setError] = useState('')
  const [newTeam, setNewTeam] = useState('')
  const teamAgents = agents.filter((a) => a.team === activeTeam)
  const team = teams.find((t) => t.key === activeTeam)

  const remove = async (a: Agent) => {
    if (!window.confirm(`${tr('Cho')} ${a.name} ${tr('nghỉ việc? Bàn làm việc sẽ bị thu hồi.')}`)) return
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

  const addTeam = async () => {
    const name = newTeam.trim()
    if (!name) return
    setError('')
    try {
      const { team } = await api.addTeam({ name })
      setNewTeam('')
      onChanged()
      onSelectTeam(team.key)
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const removeTeam = async () => {
    if (!team) return
    if (!window.confirm(`${tr('Giải thể đội')} "${team.name}"? ${tr('Toàn bộ nhân sự của đội sẽ bị xoá.')}`)) return
    setError('')
    try {
      await api.deleteTeam(team.key)
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  return (
    <div className="page">
      <div className="page-inner">
        <h2>
          👥 {tr('SƠ ĐỒ TỔ CHỨC')} — <b>{tr('đội nhóm & biên chế nhân sự AI')}</b>
        </h2>
        {/* team switcher + management */}
        <div className="team-tabs" style={{ margin: '0 0 10px', borderBottom: 'none', paddingLeft: 0 }}>
          {teams.map((t) => (
            <button
              key={t.key}
              className={`team-tab${t.key === activeTeam ? ' active' : ''}`}
              onClick={() => onSelectTeam(t.key)}
            >
              {tr(t.name)}
            </button>
          ))}
        </div>
        <div style={{ display: 'flex', gap: 6, marginBottom: 8, alignItems: 'center' }}>
          <input
            style={{ border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: '3px 6px', flex: 1 }}
            placeholder={tr('Tên đội mới, ví dụ: Chăm sóc khách hàng')}
            value={newTeam}
            onChange={(e) => setNewTeam(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addTeam()}
          />
          <button className="btn" onClick={addTeam} disabled={!newTeam.trim()}>+ {tr('Tạo đội')}</button>
          {teams.length > 1 && (
            <button className="btn" style={{ color: 'var(--danger)' }} onClick={removeTeam}>{tr('Giải thể đội')}</button>
          )}
        </div>
        {team?.description && <div style={{ color: 'var(--faint)', fontSize: 11, marginBottom: 8 }}>{tr(team.description)}</div>}
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 6 }}>
          <button className="btn" onClick={() => setEditing('new')}>+ {tr('Tuyển nhân sự vào đội')}</button>
        </div>
        {error && <div className="sysline">⚠ {error}</div>}
        <table>
          <thead>
            <tr><th>{tr('Tên')}</th><th>{tr('Vai trò')}</th><th>{tr('Loại')}</th><th>{tr('Chế độ')}</th><th /></tr>
          </thead>
          <tbody>
            {teamAgents.map((a) => (
              <tr key={a.key} style={a.enabled ? undefined : { opacity: 0.5 }}>
                <td>
                  <Avatar agentKey={a.key} size={20} />{' '}
                  <span
                    className="dot"
                    style={{ background: a.enabled ? ({ working: 'var(--working)', done: 'var(--done)', handoff: 'var(--handoff)' }[a.status] ?? 'var(--idle)') : 'var(--idle)' }}
                  />{' '}
                  <b>{tr(a.name)}</b>
                  {a.skills.length > 0 && (
                    <div style={{ color: 'var(--faint)', fontSize: 11 }}>⚡ {a.skills.join(', ')}</div>
                  )}
                </td>
                <td>{tr(a.role)}</td>
                <td>{tr(KIND_LABEL[a.kind] ?? a.kind)}</td>
                <td>
                  {!a.enabled
                    ? tr('tạm nghỉ')
                    : a.kind === 'worker'
                      ? a.auto_assign ? tr('tự nhận việc') : tr('tăng cường')
                      : tr('trực ca')}
                </td>
                <td style={{ whiteSpace: 'nowrap' }}>
                  <button className="row-btn" onClick={() => setDetail(a)}>{tr('Chi tiết')}</button>{' '}
                  <button className="row-btn" onClick={() => setEditing(a)}>{tr('Sửa')}</button>{' '}
                  {a.kind !== 'manager' && (
                    <>
                      <button className="row-btn" onClick={() => toggleEnabled(a)}>
                        {a.enabled ? tr('Tạm dừng') : tr('Kích hoạt')}
                      </button>{' '}
                      <button className="row-btn" style={{ color: 'var(--danger)' }} onClick={() => remove(a)}>
                        {tr('Xoá')}
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
            agents={teamAgents}
            team={activeTeam}
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
  team,
  onClose,
  onSaved,
}: {
  agent: Agent | null
  agents: Agent[]
  team: string
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
        const { agent: created } = await api.addAgent({ name, role, duty, kind, team })
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
          {agent ? `${tr('Sửa hồ sơ')} — ${tr(agent.name)}` : tr('Tuyển nhân sự mới')}
          <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <div className="form-grid">
          <label>{tr('Tên hiển thị')}</label>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={tr('VD: THIẾT KẾ')} />
          <label>{tr('Vai trò')}</label>
          <input value={role} onChange={(e) => setRole(e.target.value)} placeholder={tr('VD: Thiết kế & hình ảnh')} />
          <label>{tr('Nhiệm vụ cố định')}</label>
          <textarea
            rows={4}
            value={duty}
            onChange={(e) => setDuty(e.target.value)}
            placeholder={tr('Mô tả nhiệm vụ mà nhân sự này luôn đảm nhận trong quy trình…')}
          />
          <label>{tr('Loại')}</label>
          {agent ? (
            <div>{tr(KIND_LABEL[agent.kind] ?? agent.kind)} ({tr('không đổi được')})</div>
          ) : (
            <select value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="worker">{tr('Chuyên môn (worker)')}</option>
              <option value="manager" disabled={hasManager}>{tr('Trưởng phòng')} {hasManager ? tr('— đã có') : ''}</option>
              <option value="qa" disabled={hasQa}>{tr('Kiểm định')} {hasQa ? tr('— đã có') : ''}</option>
            </select>
          )}
          {(agent?.kind ?? kind) === 'worker' && (
            <>
              <label>{tr('Nhận việc')}</label>
              <label style={{ textTransform: 'none', letterSpacing: 0, fontSize: 12, color: 'var(--ink)', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={autoAssign}
                  onChange={(e) => setAutoAssign(e.target.checked)}
                />{' '}
                {tr('Tự nhận nhiệm vụ — luôn có phần việc trong mọi kế hoạch. Bỏ chọn = tăng cường (Trưởng phòng chỉ giao khi cần chuyên môn này).')}
              </label>
            </>
          )}
          <label>{tr('Skill / sub-agent nắm giữ')}</label>
          <div>
            {skills.length > 0 && (
              <div className="chips">
                {skills.map((s) => (
                  <span className="chip" key={s}>
                    ⚡ {s}
                    <button type="button" onClick={() => toggleSkill(s)} title={tr('Bỏ chọn')}>×</button>
                  </span>
                ))}
              </div>
            )}
            <input
              placeholder={tr('🔍 Tìm skill / sub-agent…')}
              value={skillQuery}
              onChange={(e) => setSkillQuery(e.target.value)}
              style={{ marginBottom: 6 }}
            />
            <div className="skill-picker">
              {inventory === null && <div style={{ color: 'var(--faint)' }}>{tr('Đang tải danh mục…')}</div>}
              {inventory && inventory.skills.length === 0 && inventory.personas.length === 0 && (
                <div style={{ color: 'var(--faint)' }}>
                  {tr('Không lấy được danh mục từ daemon — kiểm tra SenClaw daemon.')}
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
                    <div className="cap">{group === 'skills' ? tr('Skills') : tr('Sub-agents')} ({items.length})</div>
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
                  <div style={{ color: 'var(--faint)' }}>{tr('Không có mục nào khớp')} "{skillQuery}".</div>
                )}
            </div>
          </div>
        </div>
        {error && <div className="sysline">⚠ {error}</div>}
        <div style={{ marginTop: 12, textAlign: 'right' }}>
          <button className="btn" onClick={save} disabled={!name.trim()}>
            {agent ? tr('Lưu hồ sơ') : tr('Tuyển vào phòng')}
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
            {tr(agent.name)}
          </span>
          <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <div className="kv">
          <div className="k">{tr('Loại')}</div><div>{tr(KIND_LABEL[agent.kind] ?? agent.kind)}</div>
          <div className="k">{tr('Vai trò')}</div><div>{tr(agent.role) || '—'}</div>
          <div className="k">{tr('Nhiệm vụ cố định')}</div><div><Md>{tr(agent.duty) || '—'}</Md></div>
          <div className="k">{tr('Chế độ')}</div>
          <div>
            {!agent.enabled
              ? tr('Tạm nghỉ — không tham gia nhiệm vụ')
              : agent.kind === 'worker'
                ? agent.auto_assign
                  ? tr('Tự nhận nhiệm vụ — luôn có phần việc')
                  : tr('Tăng cường — chỉ được giao khi cần chuyên môn')
                : tr('Trực ca')}
          </div>
          <div className="k">{tr('Skill / sub-agent')}</div>
          <div>{agent.skills.length > 0 ? agent.skills.map((s) => `⚡ ${s}`).join('  ') : '—'}</div>
          <div className="k">{tr('Trạng thái')}</div><div>{agent.status}{agent.status_note ? ` — ${agent.status_note}` : ''}</div>
          <div className="k">{tr('Trí nhớ riêng')}</div>
          <div>
            {memErr && <span style={{ color: 'var(--faint)' }}>{tr('không đọc được')} ({memErr})</span>}
            {!memErr && mem === null && tr('đang đếm…')}
            {!memErr && mem !== null && (
              <>
                {mem.count >= 100 ? '100+' : mem.count} {tr('ký ức trong space')} <code>{mem.space}</code>
                <div style={{ color: 'var(--faint)', fontSize: 11 }}>
                  {tr('Xem chi tiết trong Knowledge (desktop app) — chọn space này ở bộ lọc.')}
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

const HISTORY_PER_PAGE = 15

/** Tab LỊCH SỬ — nhật ký mọi nhiệm vụ đã qua (trang inline như OPC HQ). */
function HistoryView() {
  const [tasks, setTasks] = useState<Task[]>([])
  const [open, setOpen] = useState<Task | null>(null)
  const [page, setPage] = useState(0)
  useEffect(() => {
    api
      .tasks(200)
      .then(({ tasks }) => setTasks(tasks))
      .catch(() => {})
  }, [])
  const pages = Math.max(1, Math.ceil(tasks.length / HISTORY_PER_PAGE))
  const p = Math.min(page, pages - 1)
  const slice = tasks.slice(p * HISTORY_PER_PAGE, p * HISTORY_PER_PAGE + HISTORY_PER_PAGE)
  return (
    <div className="page">
      <div className="page-inner">
        <h2>
          📜 {tr('LỊCH SỬ NHIỆM VỤ')} — <b>{tr('sổ ghi mọi việc đã qua tay văn phòng')}</b>
        </h2>
        {open ? (
          <div>
            <button className="row-btn" onClick={() => setOpen(null)}>← {tr('danh sách')}</button>
            <h3 style={{ fontSize: 13 }}>{open.title}</h3>
            <Md>{open.report || `*(${tr('chưa có báo cáo')})*`}</Md>
          </div>
        ) : (
          <>
            <table>
              <thead>
                <tr><th>#</th><th>{tr('Nhiệm vụ')}</th><th>{tr('Trạng thái')}</th><th>LLM</th><th>Token</th></tr>
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
                    <td>{t.tokens_in + t.tokens_out > 0 ? `~${(t.tokens_in + t.tokens_out).toLocaleString(getLang() === 'en' ? 'en-US' : 'vi-VN')}` : '—'}</td>
                  </tr>
                ))}
                {tasks.length === 0 && (
                  <tr><td colSpan={5}>{tr('Chưa có nhiệm vụ nào — giao việc đầu tiên cho phòng đi Sếp!')}</td></tr>
                )}
              </tbody>
            </table>
            {pages > 1 && (
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'center', marginTop: 10 }}>
                <button className="btn" disabled={p === 0} onClick={() => setPage(p - 1)}>← {tr('Trước')}</button>
                <span style={{ fontSize: 12 }}>{tr('Trang')} {p + 1}/{pages} · {tasks.length} {tr('nhiệm vụ')}</span>
                <button className="btn" disabled={p >= pages - 1} onClick={() => setPage(p + 1)}>{tr('Sau')} →</button>
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
          {tr('Kế toán')} <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        {stats ? (
          <div className="kv">
            <div className="k">{tr('Tổng nhiệm vụ')}</div><div>{stats.tasksTotal}</div>
            <div className="k">{tr('Đã hoàn thành')}</div><div>{stats.tasksDone}</div>
            <div className="k">{tr('Lượt gọi LLM')}</div><div>{stats.llmCalls}</div>
            <div className="k">{tr('Token đã dùng (ước tính)')}</div>
            <div>
              {stats.tokensIn + stats.tokensOut > 0
                ? `${(stats.tokensIn + stats.tokensOut).toLocaleString(getLang() === 'en' ? 'en-US' : 'vi-VN')} (${tr('vào')} ${stats.tokensIn.toLocaleString(getLang() === 'en' ? 'en-US' : 'vi-VN')} · ${tr('ra')} ${stats.tokensOut.toLocaleString(getLang() === 'en' ? 'en-US' : 'vi-VN')})`
                : '0'}
            </div>
            <div className="k">{tr('Model gần nhất')}</div><div>{stats.lastModel || '—'}</div>
            <div className="k">{tr('Lương nhân sự')}</div><div>{tr('0 ₫ (agent không nhận lương 😜)')}</div>
          </div>
        ) : (
          <div>{tr('Đang tải…')}</div>
        )}
      </div>
    </div>
  )
}

function SettingsPanel({
  llmOk,
  rotation,
  onRotate,
  lang,
  onSetLang,
  onClose,
}: {
  llmOk: boolean | null
  rotation: number
  onRotate: (r: number) => void
  lang: Lang
  onSetLang: (l: Lang) => void
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
      setWsMsg(`✓ ${tr('đã lưu')}`)
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
          {tr('Cài đặt')} <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <div className="kv">
          <div className="k">{tr('Ngôn ngữ')}</div>
          <div style={{ display: 'flex', gap: 6 }}>
            <button
              className="btn"
              style={lang === 'vi' ? { background: 'var(--ink)', color: 'var(--paper)' } : undefined}
              onClick={() => onSetLang('vi')}
            >
              🇻🇳 Tiếng Việt
            </button>
            <button
              className="btn"
              style={lang === 'en' ? { background: 'var(--ink)', color: 'var(--paper)' } : undefined}
              onClick={() => onSetLang('en')}
            >
              🇬🇧 English
            </button>
          </div>
          <div className="k">{tr('Workspace folder')}</div>
          <div>
            <div style={{ display: 'flex', gap: 6 }}>
              <input
                style={{ flex: 1, border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: '4px 6px' }}
                value={wsDir}
                onChange={(e) => setWsDir(e.target.value)}
                placeholder={tr('~/Documents/ai-office hoặc đường dẫn tuyệt đối')}
              />
              <button className="btn" onClick={() => setPickerOpen(true)}>{tr('Chọn…')}</button>
              <button className="btn" onClick={saveWs}>{tr('Lưu')}</button>
            </div>
            <div style={{ color: 'var(--faint)', fontSize: 11, marginTop: 4 }}>
              {tr('Kho tài liệu chung của phòng: Sếp bỏ tệp tham khảo vào đây (mở bằng Finder), nhân sự sẽ đọc khi làm việc và ghi kết quả vào')} <code>task-&lt;id&gt;/…</code>.
              {' '}{tr('Để trống rồi Lưu = quay về thư mục mặc định.')}
              {settings && (
                <> {tr('Hiện có')} <b>{settings.workspaceFiles}</b> {tr('tệp')}{settings.workspaceIsDefault ? ` (${tr('mặc định')})` : ''}.</>
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
          <div className="k">{tr('Góc nhìn văn phòng')}</div>
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
              {tr('Xoay tự do 360° quanh tâm sàn — kéo chuột trái/phải ngay trên khung mô phỏng, hoặc chỉnh bằng thanh trượt / nút góc ở đây.')}
            </div>
          </div>
          <div className="k feat-head">{tr('Chức năng phòng')}</div>
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
                    <b>{tr(label)}</b>
                    <span className="feat-desc">{tr(desc)}</span>
                  </span>
                </label>
              )
            })}
          </div>
          <div className="k">{tr('Vận hành')}</div>
          <div>
            {tr('Mỗi agent xử lý thật phần việc của mình qua LLM của SenClaw daemon.')}
            {llmOk === false && <span style={{ color: 'var(--danger)' }}> {tr('Hiện không kết nối được daemon LLM.')}</span>}
            {llmOk === true && <span style={{ color: 'var(--done)' }}> {tr('Daemon LLM sẵn sàng.')}</span>}
          </div>
          <div className="k">{tr('MCP cho agent ngoài')}</div>
          <div>
            {tr('Server')} <code>ai-office-mcp</code> — {tr('agent SenClaw có thể giao việc bằng')}{' '}
            <code>office_create_task</code> {tr('và lấy kết quả bằng')} <code>office_get_report</code>.
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
                setWsMsg(`✓ ${tr('đã lưu')}`)
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
        [`🏠 ${tr('Home')}`, listing.home],
        [`📄 ${tr('Documents')}`, `${listing.home}/Documents`],
        [`🖥 ${tr('Desktop')}`, `${listing.home}/Desktop`],
        [`⬇ ${tr('Downloads')}`, `${listing.home}/Downloads`],
      ]
    : []

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(560px, 90vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          {tr('Chọn workspace folder')}
          <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
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
            <div className="dir-row" onClick={() => load(listing.parent!)}>⬆ .. ({tr('lên thư mục cha')})</div>
          )}
          {listing?.dirs.map((d) => (
            <div className="dir-row" key={d} onClick={() => load(`${listing.path}/${d}`)}>
              📁 {d}
            </div>
          ))}
          {listing && listing.dirs.length === 0 && (
            <div style={{ color: 'var(--faint)', padding: 6 }}>({tr('không có thư mục con')})</div>
          )}
        </div>
        <div style={{ marginTop: 10, textAlign: 'right' }}>
          <button className="btn" disabled={!listing} onClick={() => listing && onSelect(listing.path)}>
            ✓ {tr('Chọn thư mục này')}
          </button>
        </div>
      </div>
    </div>
  )
}
