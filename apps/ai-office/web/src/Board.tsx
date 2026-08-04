import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import { Md } from './Feed'
import { tr } from './i18n'
import type { Board, Goal, Step, Task, Team } from './types'

/** BẢNG VIỆC — kanban kiểu trụ sở điều hành: AI làm, Sếp duyệt.
 *  HỘP VIỆC (chưa chạy + lỗi) → ĐANG LÀM → CHỜ SẾP DUYỆT → HOÀN TẤT. */
export function BoardView({ teams }: { teams: Team[] }) {
  const [board, setBoard] = useState<Board | null>(null)
  const [goals, setGoals] = useState<Goal[]>([])
  const [openTask, setOpenTask] = useState<Task | null>(null)
  const [newOpen, setNewOpen] = useState(false)

  const load = useCallback(async () => {
    try {
      const [b, g] = await Promise.all([api.board(), api.goals()])
      setBoard(b)
      setGoals(g.goals)
      // Giữ modal đang mở đồng bộ trạng thái mới nhất của việc đó.
      setOpenTask((prev) => {
        if (!prev) return prev
        const all = [...b.columns.inbox, ...b.columns.doing, ...b.columns.waiting, ...b.columns.done]
        return all.find((t) => t.id === prev.id) ?? prev
      })
    } catch {
      /* backend briefly unavailable */
    }
  }, [])

  useEffect(() => {
    load()
    const t = setInterval(load, 2000)
    return () => clearInterval(t)
  }, [load])

  const teamName = (key: string) => tr(teams.find((t) => t.key === key)?.name ?? key.toUpperCase())
  const goalOf = (t: Task) => (t.goal_id != null ? board?.goals[String(t.goal_id)] : undefined)

  const columns: { key: keyof Board['columns']; title: string; klass: string }[] = [
    { key: 'inbox', title: tr('HỘP VIỆC'), klass: 'col-inbox' },
    { key: 'doing', title: tr('ĐANG LÀM'), klass: 'col-doing' },
    { key: 'waiting', title: tr('CHỜ SẾP DUYỆT'), klass: 'col-waiting' },
    { key: 'done', title: tr('HOÀN TẤT'), klass: 'col-done' },
  ]

  return (
    <div className="board">
      <div className="board-head">
        <span className="board-title">
          {tr('BẢNG VIỆC')} — <b>{tr('AI làm, Sếp duyệt')}</b>
        </span>
        <button className="btn hq-primary" onClick={() => setNewOpen(true)}>
          + {tr('GIAO VIỆC MỚI')}
        </button>
      </div>
      <div className="board-cols">
        {columns.map((c) => {
          const tasks = board?.columns[c.key] ?? []
          return (
            <div className={`board-col ${c.klass}`} key={c.key}>
              <div className="board-col-head">
                <span>{c.title}</span>
                <span className="board-count">{tasks.length}</span>
              </div>
              <div className="board-col-body">
                {tasks.map((t) => {
                  const g = goalOf(t)
                  return (
                    <div
                      className={`board-card${t.status === 'error' ? ' is-error' : ''}`}
                      key={t.id}
                      onClick={() => setOpenTask(t)}
                    >
                      <div className="bc-title">{t.title}</div>
                      <div className="bc-meta">
                        <span className="bc-team">{teamName(t.team)}</span>
                        {t.status === 'error' && <span className="bc-flag err">⚠ {tr('lỗi')}</span>}
                        {t.status === 'pending' && c.key === 'doing' && (
                          <span className="bc-flag">⏳ {tr('hàng đợi')}</span>
                        )}
                        {['planning', 'running', 'review'].includes(t.status) && (
                          <span className="bc-flag run pulse">● {tr('đang chạy')}</span>
                        )}
                        {t.approval === 'returned' && t.status !== 'done' && (
                          <span className="bc-flag">↩ {tr('làm lại')}</span>
                        )}
                      </div>
                      <div className="bc-goal">
                        {g ? (
                          <span className="bc-goal-chip" title={g.title}>🎯 {g.title}</span>
                        ) : (
                          <span className="bc-goal-chip off">⚠ {tr('lạc hướng')}</span>
                        )}
                      </div>
                    </div>
                  )
                })}
                {tasks.length === 0 && <div className="board-empty">{tr('(trống)')}</div>}
              </div>
            </div>
          )
        })}
      </div>
      {newOpen && (
        <NewBoardTaskDialog
          teams={teams}
          goals={goals.filter((g) => !g.archived)}
          onClose={() => setNewOpen(false)}
          onCreated={() => {
            setNewOpen(false)
            load()
          }}
        />
      )}
      {openTask && (
        <TaskModal
          task={openTask}
          teams={teams}
          goals={goals}
          onClose={() => setOpenTask(null)}
          onChanged={load}
        />
      )}
    </div>
  )
}

/** Giao việc mới từ bảng: chọn đội, gắn mục tiêu, chạy ngay / để Hộp việc. */
function NewBoardTaskDialog({
  teams,
  goals,
  onClose,
  onCreated,
}: {
  teams: Team[]
  goals: Goal[]
  onClose: () => void
  onCreated: () => void
}) {
  const [title, setTitle] = useState('')
  const [team, setTeam] = useState(teams[0]?.key ?? '')
  const [goalId, setGoalId] = useState<number>(0)
  const [start, setStart] = useState(true)
  const [err, setErr] = useState('')

  const submit = async () => {
    if (!title.trim() || !team) return
    setErr('')
    try {
      await api.createTask(title.trim(), team, {
        start,
        ...(goalId ? { goalId } : {}),
      })
      onCreated()
    } catch (e) {
      setErr(String((e as Error).message))
    }
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(560px, 92vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          {tr('GIAO VIỆC MỚI')} <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <div className="form-grid">
          <label>{tr('Nội dung việc')}</label>
          <textarea
            rows={3}
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => (e.key === 'Enter' && (e.metaKey || e.ctrlKey) ? submit() : undefined)}
            placeholder={tr('Ví dụ: khảo sát 5 kênh TikTok cùng ngách để rút công thức video')}
          />
          <label>{tr('Đội xử lý')}</label>
          <select value={team} onChange={(e) => setTeam(e.target.value)}>
            {teams.map((t) => (
              <option key={t.key} value={t.key}>{tr(t.name)}</option>
            ))}
          </select>
          <label>{tr('Mục tiêu')}</label>
          <select value={goalId} onChange={(e) => setGoalId(Number(e.target.value))}>
            <option value={0}>{tr('— không gắn (lạc hướng) —')}</option>
            {goals.map((g) => (
              <option key={g.id} value={g.id}>🎯 {g.title}</option>
            ))}
          </select>
          <label>{tr('Chạy')}</label>
          <label style={{ textTransform: 'none', letterSpacing: 0, fontSize: 12, color: 'var(--ink)', cursor: 'pointer' }}>
            <input type="checkbox" checked={start} onChange={(e) => setStart(e.target.checked)} />{' '}
            {tr('Chạy ngay (bỏ chọn = để trong Hộp việc, chạy sau)')}
          </label>
        </div>
        {err && <div className="sysline">⚠ {err}</div>}
        <div style={{ marginTop: 12, textAlign: 'right' }}>
          <button className="btn" onClick={submit} disabled={!title.trim()}>
            {start ? tr('Giao việc') : tr('Đưa vào Hộp việc')}
          </button>
        </div>
      </div>
    </div>
  )
}

const STATUS_LABEL: Record<string, string> = {
  inbox: 'Hộp việc — chưa chạy',
  pending: 'Đang xếp hàng đợi',
  planning: 'Trưởng nhóm đang phân công',
  running: 'Cả đội đang làm',
  review: 'Kiểm định đang soát',
  done: 'AI đã xong',
  error: 'Lỗi — cần Sếp xử lý',
}

/** Chi tiết một việc trên bảng + hành động của Sếp theo trạng thái. */
function TaskModal({
  task,
  teams,
  goals,
  onClose,
  onChanged,
}: {
  task: Task
  teams: Team[]
  goals: Goal[]
  onClose: () => void
  onChanged: () => void
}) {
  const [steps, setSteps] = useState<Step[]>([])
  const [report, setReport] = useState(task.report)
  const [returnOpen, setReturnOpen] = useState(false)
  const [note, setNote] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api
      .task(task.id)
      .then(({ task: t, steps }) => {
        setSteps(steps)
        setReport(t.report)
      })
      .catch(() => {})
  }, [task.id, task.status, task.approval])

  const act = async (fn: () => Promise<unknown>, close = false) => {
    setErr('')
    setBusy(true)
    try {
      await fn()
      onChanged()
      if (close) onClose()
    } catch (e) {
      setErr(String((e as Error).message))
    } finally {
      setBusy(false)
    }
  }

  const teamName = tr(teams.find((t) => t.key === task.team)?.name ?? task.team.toUpperCase())
  const waiting = task.status === 'done' && task.approval === 'waiting'
  const approved = task.status === 'done' && task.approval !== 'waiting'
  const runnable = task.status === 'inbox' || task.status === 'error'
  const running = ['pending', 'planning', 'running', 'review'].includes(task.status)

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>
          #{task.id} · {tr('CHI TIẾT VIỆC')}
          <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <h3 style={{ fontSize: 13, margin: '0 0 8px' }}>{task.title}</h3>
        <div className="kv" style={{ marginBottom: 10 }}>
          <div className="k">{tr('Đội xử lý')}</div>
          <div>{teamName}</div>
          <div className="k">{tr('Trạng thái')}</div>
          <div>
            {waiting && <b style={{ color: 'var(--working)' }}>📥 {tr('Chờ Sếp duyệt')}</b>}
            {approved && <b style={{ color: 'var(--done)' }}>✓ {tr('Hoàn tất — Sếp đã duyệt')}</b>}
            {!waiting && !approved && tr(STATUS_LABEL[task.status] ?? task.status)}
          </div>
          <div className="k">{tr('Mục tiêu')}</div>
          <div>
            <select
              value={task.goal_id ?? 0}
              disabled={busy}
              onChange={(e) => act(() => api.updateTask(task.id, { goalId: Number(e.target.value) }))}
              style={{ maxWidth: '100%', border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: '3px 6px' }}
            >
              <option value={0}>{tr('— không gắn (lạc hướng) —')}</option>
              {goals.filter((g) => !g.archived || g.id === task.goal_id).map((g) => (
                <option key={g.id} value={g.id}>🎯 {g.title}</option>
              ))}
            </select>
          </div>
          {task.boss_note && (
            <>
              <div className="k">{tr('Ghi chú Sếp')}</div>
              <div>↩ {task.boss_note}</div>
            </>
          )}
        </div>

        {running && (
          <div className="hint" style={{ marginBottom: 10 }}>
            {tr('Việc đang chạy — xem cả đội làm việc trực tiếp ở tab VĂN PHÒNG.')}
          </div>
        )}

        {steps.length > 0 && (
          <>
            <div className="cap" style={{ margin: '6px 0' }}>{tr('Phân công')}</div>
            <table style={{ marginBottom: 10 }}>
              <tbody>
                {steps.map((s) => (
                  <tr key={s.id}>
                    <td style={{ whiteSpace: 'nowrap' }}>{s.agent_key.replace(/^[^-]*-/, '').replace(/-/g, ' ').toUpperCase()}</td>
                    <td>{s.title}</td>
                    <td style={{ whiteSpace: 'nowrap' }}>
                      {s.status === 'done' ? '✓' : s.status === 'working' ? '●' : '…'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}

        {report && (
          <>
            <div className="cap" style={{ margin: '6px 0' }}>{tr('Báo cáo tổng hợp')}</div>
            <div className="msg report" style={{ maxWidth: '100%' }}>
              <div className="box">
                <Md>{report}</Md>
              </div>
            </div>
          </>
        )}

        {err && <div className="sysline">⚠ {err}</div>}

        <div className="task-actions">
          {runnable && (
            <button className="btn hq-primary" disabled={busy} onClick={() => act(() => api.startTask(task.id))}>
              ▶ {task.status === 'error' ? tr('Chạy lại') : tr('Chạy việc này')}
            </button>
          )}
          {waiting && !returnOpen && (
            <>
              <button className="btn hq-primary" disabled={busy} onClick={() => act(() => api.approveTask(task.id), true)}>
                ✓ {tr('DUYỆT — nghiệm thu')}
              </button>
              <button className="btn" disabled={busy} onClick={() => setReturnOpen(true)}>
                ↩ {tr('TRẢ LẠI')}
              </button>
            </>
          )}
          {!running && (
            <button
              className="btn"
              style={{ color: 'var(--danger)', marginLeft: 'auto' }}
              disabled={busy}
              onClick={() => {
                if (window.confirm(tr('Xoá việc này khỏi bảng? Nhật ký của nó cũng bị xoá.')))
                  act(() => api.deleteTask(task.id), true)
              }}
            >
              🗑 {tr('Xoá')}
            </button>
          )}
        </div>
        {returnOpen && (
          <div style={{ marginTop: 10 }}>
            <textarea
              rows={3}
              autoFocus
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder={tr('Sếp muốn sửa gì? VD: phần định giá thiếu đối thủ X, viết lại ngắn hơn…')}
              style={{ width: '100%', border: '1px solid var(--line-strong)', background: 'var(--panel)', padding: 8 }}
            />
            <div style={{ marginTop: 6, textAlign: 'right', display: 'flex', gap: 6, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setReturnOpen(false)}>{tr('Thôi')}</button>
              <button
                className="btn"
                disabled={busy || !note.trim()}
                onClick={() => act(() => api.returnTask(task.id, note.trim()), true)}
              >
                ↩ {tr('Trả lại — đội làm lại theo ghi chú')}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
