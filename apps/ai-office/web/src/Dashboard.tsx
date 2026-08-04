import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import { Md } from './Feed'
import { tr, getLang } from './i18n'
import type { Dashboard, Goal, Meeting } from './types'

/** BÀN LÀM VIỆC CỦA SẾP — dashboard điều hành kiểu "trụ sở công ty một
 *  người": 5 thẻ KPI, họp sáng / họp tối với Giám đốc vận hành (biên bản do
 *  LLM viết từ toàn cảnh văn phòng) và mục tiêu quý (OKR rút gọn). */
export function DashboardView({ onOpenBoard }: { onOpenBoard: () => void }) {
  const [dash, setDash] = useState<Dashboard | null>(null)
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const [goals, setGoals] = useState<Goal[]>([])
  const [meetingBusy, setMeetingBusy] = useState<'' | 'morning' | 'evening'>('')
  const [error, setError] = useState('')

  const load = useCallback(async () => {
    try {
      const [d, m, g] = await Promise.all([api.dashboard(), api.meetings(), api.goals()])
      setDash(d)
      setMeetings(m.meetings)
      setGoals(g.goals)
    } catch {
      /* backend briefly unavailable */
    }
  }, [])

  useEffect(() => {
    load()
    const t = setInterval(load, 4000)
    return () => clearInterval(t)
  }, [load])

  const runMeeting = async (kind: 'morning' | 'evening') => {
    if (meetingBusy) return
    setMeetingBusy(kind)
    setError('')
    try {
      await api.runMeeting(kind)
      await load()
    } catch (e) {
      setError(String((e as Error).message))
    } finally {
      setMeetingBusy('')
    }
  }

  const locale = getLang() === 'en' ? 'en-US' : 'vi-VN'
  const dayLabel = new Date().toLocaleDateString(locale, {
    weekday: 'long',
    day: 'numeric',
    month: 'numeric',
    year: 'numeric',
  })
  const fmt = (n: number) => n.toLocaleString(locale)
  const alignPct = dash?.alignment.percent ?? null

  return (
    <div className="hq">
      <div className="hq-head">
        {tr('BÀN LÀM VIỆC CỦA SẾP')} — <b>{dayLabel}</b> · AI OFFICE
      </div>

      <div className="hq-cards">
        <div className="hq-card">
          <div className="cap">{tr('ĐỘ BÁM HƯỚNG')}</div>
          <div className="val">
            {alignPct === null ? '—' : <>{alignPct}<small>%</small></>}
          </div>
          <div className="bar">
            <span style={{ width: `${alignPct ?? 0}%` }} />
          </div>
          <div className="note">
            {alignPct === null
              ? tr('Chưa có việc đang mở trên bảng')
              : `${dash!.alignment.aligned}/${dash!.alignment.open} ${tr('việc đang bám mục tiêu')}`}
          </div>
        </div>
        <div className="hq-card">
          <div className="cap">{tr('MỤC TIÊU QUÝ')}</div>
          <div className="val">
            {dash?.goals.count ? (
              <>{dash.goals.avgProgress}<small>% · {dash.goals.count} {tr('mục tiêu')}</small></>
            ) : (
              '—'
            )}
          </div>
          <div className="bar">
            <span style={{ width: `${dash?.goals.count ? dash.goals.avgProgress : 0}%` }} />
          </div>
          <div className="note">{tr('Tiến độ trung bình các mục tiêu')}</div>
        </div>
        <div className="hq-card" onClick={onOpenBoard} style={{ cursor: 'pointer' }} title={tr('Mở Bảng việc')}>
          <div className="cap">{tr('CHỜ SẾP DUYỆT')}</div>
          <div className="val" style={dash?.waiting ? { color: 'var(--working)' } : { color: 'var(--done)' }}>
            {dash?.waiting ?? 0}<small> {tr('việc')}</small>
          </div>
          <div className="note">
            {dash?.waiting
              ? tr('Bấm để nghiệm thu trên Bảng việc')
              : tr('Bàn Sếp đang sạch')}
          </div>
        </div>
        <div className="hq-card">
          <div className="cap">{tr('NHỊP ĐIỀU HÀNH')}</div>
          <div className="val">
            {dash?.streak.days ?? 0}<small> {tr('ngày liên tiếp')}</small>
          </div>
          <div className="note">
            {dash?.streak.morningToday ? `${tr('Hôm nay đã họp sáng')} ✓` : tr('Hôm nay chưa họp sáng')}
          </div>
        </div>
        <div className="hq-card">
          <div className="cap">{tr('CHI PHÍ AI (THÁNG)')}</div>
          <div className="val">
            ~{fmt(dash?.budget.monthTokens ?? 0)}<small> token</small>
          </div>
          <div className="note">
            {dash?.budget.openTasks ?? 0} {tr('việc đang mở trên bảng')}
          </div>
        </div>
      </div>

      <div className="hq-actions">
        <button
          className="btn hq-primary"
          disabled={!!meetingBusy}
          onClick={() => runMeeting('morning')}
        >
          ☀ {tr('HỌP SÁNG VỚI GIÁM ĐỐC VẬN HÀNH')}
        </button>
        <button className="btn" disabled={!!meetingBusy} onClick={() => runMeeting('evening')}>
          🌙 {tr('HỌP TỐI — TỔNG KẾT NGÀY')}
        </button>
        {meetingBusy && (
          <span className="hq-running pulse">
            {tr('Giám đốc vận hành đang chuẩn bị biên bản…')}
          </span>
        )}
      </div>
      {error && <div className="sysline">⚠ {error}</div>}

      <GoalsSection goals={goals} onChanged={load} />

      <div className="hq-minutes">
        {meetings.length === 0 && !meetingBusy && (
          <div className="hq-empty">
            {tr('Chưa có biên bản họp nào.')}
            <br />
            {tr('Bấm ☀ HỌP SÁNG để Giám đốc vận hành điểm tình hình công ty và đề xuất 3 ưu tiên hôm nay.')}
          </div>
        )}
        {meetings.map((m) => (
          <div className="hq-minute" key={m.id}>
            <div className="hq-minute-head">
              {m.kind === 'morning' ? '☀' : '🌙'}{' '}
              {m.kind === 'morning' ? tr('BIÊN BẢN HỌP SÁNG') : tr('BIÊN BẢN HỌP TỐI')} — {m.day} —{' '}
              {tr('GIÁM ĐỐC VẬN HÀNH')}
            </div>
            <Md>{m.content}</Md>
          </div>
        ))}
      </div>
    </div>
  )
}

/** MỤC TIÊU QUÝ — mọi việc trên bảng đều nên truy được về đây. */
function GoalsSection({ goals, onChanged }: { goals: Goal[]; onChanged: () => void }) {
  const [editing, setEditing] = useState<Goal | 'new' | null>(null)
  const [error, setError] = useState('')
  const active = goals.filter((g) => !g.archived)

  const toggleKr = async (g: Goal, idx: number) => {
    const krs = g.key_results.map((k, i) => (i === idx ? { ...k, done: !k.done } : k))
    setError('')
    try {
      await api.updateGoal(g.id, { keyResults: krs })
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const archive = async (g: Goal) => {
    setError('')
    try {
      await api.updateGoal(g.id, { archived: true })
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  const remove = async (g: Goal) => {
    if (!window.confirm(`${tr('Xoá mục tiêu')} "${g.title}"? ${tr('Việc đang gắn sẽ thành lạc hướng.')}`)) return
    setError('')
    try {
      await api.deleteGoal(g.id)
      onChanged()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  return (
    <div className="hq-goals">
      <div className="hq-section-head">
        <span>
          🎯 {tr('MỤC TIÊU QUÝ')} — {tr('mọi việc trên bảng đều truy được về đây')}
        </span>
        <button className="btn" onClick={() => setEditing('new')}>
          + {tr('Thêm mục tiêu')}
        </button>
      </div>
      {error && <div className="sysline">⚠ {error}</div>}
      {active.length === 0 && (
        <div className="hq-empty" style={{ padding: '10px 0' }}>
          {tr('Chưa có mục tiêu quý — đặt mục tiêu để đo ĐỘ BÁM HƯỚNG của bảng việc.')}
        </div>
      )}
      {active.map((g) => (
        <div className="goal-row" key={g.id}>
          <div className="goal-head">
            <b>{g.title}</b>
            <span className="goal-meta">
              {g.quarter && <>{g.quarter} · </>}
              {g.progress}% · {g.openTaskCount ?? 0} {tr('việc đang mở')}
            </span>
          </div>
          <div className="bar">
            <span style={{ width: `${g.progress}%` }} />
          </div>
          <div className="goal-krs">
            {g.key_results.map((k, i) => (
              <label key={i} className={k.done ? 'kr done' : 'kr'}>
                <input type="checkbox" checked={k.done} onChange={() => toggleKr(g, i)} />
                <span>{k.text}</span>
              </label>
            ))}
            {g.key_results.length === 0 && (
              <span style={{ color: 'var(--faint)', fontSize: 11 }}>
                ({tr('chưa có kết quả then chốt — bấm Sửa để thêm')})
              </span>
            )}
          </div>
          <div className="goal-actions">
            <button className="row-btn" onClick={() => setEditing(g)}>{tr('Sửa')}</button>
            <button className="row-btn" onClick={() => archive(g)}>{tr('Lưu trữ')}</button>
            <button className="row-btn" style={{ color: 'var(--danger)' }} onClick={() => remove(g)}>
              {tr('Xoá')}
            </button>
          </div>
        </div>
      ))}
      {editing && (
        <GoalDialog
          goal={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            onChanged()
          }}
        />
      )}
    </div>
  )
}

function GoalDialog({
  goal,
  onClose,
  onSaved,
}: {
  goal: Goal | null
  onClose: () => void
  onSaved: () => void
}) {
  const [title, setTitle] = useState(goal?.title ?? '')
  const [quarter, setQuarter] = useState(goal?.quarter ?? defaultQuarter())
  const [krText, setKrText] = useState(goal?.key_results.map((k) => k.text).join('\n') ?? '')
  const [error, setError] = useState('')

  const save = async () => {
    setError('')
    // Giữ trạng thái ✓ của KR trùng tên khi sửa; KR mới thêm mặc định ☐.
    const lines = krText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean)
    const keyResults = lines.map((text) => ({
      text,
      done: goal?.key_results.find((k) => k.text === text)?.done ?? false,
    }))
    try {
      if (goal) {
        await api.updateGoal(goal.id, { title, quarter, keyResults })
      } else {
        await api.addGoal({ title, quarter, keyResults })
      }
      onSaved()
    } catch (e) {
      setError(String((e as Error).message))
    }
  }

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal" style={{ width: 'min(520px, 92vw)' }} onClick={(e) => e.stopPropagation()}>
        <h2>
          {goal ? tr('Sửa mục tiêu quý') : tr('Thêm mục tiêu quý')}
          <button className="btn" onClick={onClose}>{tr('Đóng')}</button>
        </h2>
        <div className="form-grid">
          <label>{tr('Mục tiêu')}</label>
          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={tr('VD: Đạt 30 triệu doanh thu/tháng từ khoá học')}
          />
          <label>{tr('Quý')}</label>
          <input value={quarter} onChange={(e) => setQuarter(e.target.value)} placeholder="Q3/2026" />
          <label>{tr('Kết quả then chốt')}</label>
          <textarea
            rows={4}
            value={krText}
            onChange={(e) => setKrText(e.target.value)}
            placeholder={tr('Mỗi dòng một kết quả đo được, VD:\n50 học viên mới trong quý\nChuỗi email 5 thư chạy tự động')}
          />
        </div>
        {error && <div className="sysline">⚠ {error}</div>}
        <div style={{ marginTop: 12, textAlign: 'right' }}>
          <button className="btn" onClick={save} disabled={!title.trim()}>
            {goal ? tr('Lưu mục tiêu') : tr('Đặt mục tiêu')}
          </button>
        </div>
      </div>
    </div>
  )
}

function defaultQuarter(): string {
  const d = new Date()
  return `Q${Math.floor(d.getMonth() / 3) + 1}/${d.getFullYear()}`
}
