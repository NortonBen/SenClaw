import { useEffect, useState, type ReactNode } from 'react'
import { api, type DayInfo, type MonthData, type Verdict } from './api'

const DOW_SHORT = ['T2', 'T3', 'T4', 'T5', 'T6', 'T7', 'CN']

function pad(n: number) {
  return String(n).padStart(2, '0')
}
function iso(y: number, m: number, d: number) {
  return `${y}-${pad(m)}-${pad(d)}`
}

function verdictHead(v: Verdict): string {
  return v === 'tot' ? 'good' : v === 'xau' ? 'bad' : 'mid'
}
function verdictText(v: Verdict): string {
  return v === 'tot' ? 'NGÀY TỐT' : v === 'xau' ? 'NGÀY XẤU' : 'BÌNH THƯỜNG'
}

export default function App() {
  const [date, setDate] = useState<string>('') // YYYY-MM-DD, '' = today
  const [day, setDay] = useState<DayInfo | null>(null)
  const [month, setMonth] = useState<{ y: number; m: number } | null>(null)
  const [monthData, setMonthData] = useState<MonthData | null>(null)
  const [err, setErr] = useState('')

  // Initial load: backend's "today".
  useEffect(() => {
    api
      .day()
      .then((d) => {
        setDay(d)
        setDate(d.solar_date)
        setMonth({ y: d.solar_year, m: d.solar_month })
      })
      .catch((e) => setErr(String(e)))
  }, [])

  // Load a specific day when `date` changes.
  useEffect(() => {
    if (!date) return
    api.day(date).then(setDay).catch((e) => setErr(String(e)))
  }, [date])

  // Load the month grid when month changes.
  useEffect(() => {
    if (!month) return
    api.month(month.y, month.m).then(setMonthData).catch((e) => setErr(String(e)))
  }, [month])

  const [todayRef, setTodayRef] = useState('')
  useEffect(() => {
    // Remember the real "today" from the very first backend response.
    if (!todayRef && day) setTodayRef(day.solar_date)
  }, [day, todayRef])

  function goToday() {
    api.day().then((d) => {
      setDay(d)
      setDate(d.solar_date)
      setMonth({ y: d.solar_year, m: d.solar_month })
    })
  }
  function shiftDay(delta: number) {
    if (!day) return
    const dt = new Date(day.solar_year, day.solar_month - 1, day.solar_day + delta)
    setDate(iso(dt.getFullYear(), dt.getMonth() + 1, dt.getDate()))
  }
  function shiftMonth(delta: number) {
    if (!month) return
    let { y, m } = month
    m += delta
    if (m < 1) {
      m = 12
      y--
    } else if (m > 12) {
      m = 1
      y++
    }
    setMonth({ y, m })
  }

  if (err && !day)
    return (
      <div className="app">
        <div className="err">Lỗi: {err}</div>
      </div>
    )
  if (!day) return <div className="app loading">Đang tải lịch âm…</div>

  return (
    <div className="app">
      <div className="topbar">
        <div className="brand">
          <span className="moon">🌙</span>
          <div>
            Lịch Âm · Luna Calendar
            <small>Xem ngày tốt xấu · Âm lịch Việt Nam</small>
          </div>
        </div>
        <div className="spacer" />
        <div className="datepick">
          <button className="btn ghost" onClick={() => shiftDay(-1)} title="Hôm trước">
            ‹
          </button>
          <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          <button className="btn ghost" onClick={() => shiftDay(1)} title="Hôm sau">
            ›
          </button>
          <button className="btn primary" onClick={goToday}>
            Hôm nay
          </button>
        </div>
      </div>

      <div className="layout">
        <DayCard day={day} />
        <div>
          {monthData && month && (
            <MonthGrid
              data={monthData}
              selected={day.solar_date}
              today={todayRef}
              onPick={(d) => setDate(d)}
              onPrev={() => shiftMonth(-1)}
              onNext={() => shiftMonth(1)}
              title={`Tháng ${month.m} năm ${month.y}`}
            />
          )}
          <LunarLookup onResult={(d) => setDate(d)} />
          <AdviseTool date={day.solar_date} />
        </div>
      </div>

      <div className="footer">
        Âm lịch tính theo thuật toán Hồ Ngọc Đức (múi giờ GMT+7). Thông tin ngày tốt xấu, giờ
        hoàng đạo, hướng xuất hành mang tính tham khảo văn hoá truyền thống.
      </div>
    </div>
  )
}

function DayCard({ day }: { day: DayInfo }) {
  const head = verdictHead(day.verdict)
  return (
    <div className="card">
      <div className={`day-head ${head}`}>
        <div className="verdict-badge">{verdictText(day.verdict)}</div>
        <div className="solar-big">{day.solar_day}</div>
        <div className="solar-sub">
          {day.weekday} · Tháng {day.solar_month} năm {day.solar_year}
        </div>
        <div className="lunar-row">
          <span className="lunar-big">
            Âm lịch {day.lunar_date} {day.lunar_leap ? '(nhuận)' : ''}
          </span>
          <span>
            năm {day.year_can_chi} ({day.year_animal})
          </span>
        </div>
      </div>

      <div className="canchi-row">
        <div className="canchi-cell">
          <div className="k">Ngày</div>
          <div className="v">{day.day_can_chi}</div>
        </div>
        <div className="canchi-cell">
          <div className="k">Tháng</div>
          <div className="v">{day.month_can_chi}</div>
        </div>
        <div className="canchi-cell">
          <div className="k">Năm</div>
          <div className="v">{day.year_can_chi}</div>
        </div>
      </div>

      <div className="section-title">📜 Xem ngày tốt xấu hôm nay</div>
      <div className="advice-box">
        <span className={`tag ${day.hoang_dao ? 'good' : 'bad'}`}>
          {day.hoang_dao ? 'Ngày Hoàng Đạo' : 'Ngày Hắc Đạo'} — {day.day_god}
        </span>
        {day.warnings.length > 0 && (
          <span style={{ marginLeft: 8 }}>
            {day.warnings.map((w) => (
              <span key={w} className="tag warn">
                {w}
              </span>
            ))}
          </span>
        )}
        <div style={{ marginTop: 8 }}>{day.advice}</div>
      </div>

      <div className="rows">
        <Row label="Tiết khí" value={day.tiet_khi} />
        <Row label="Ngũ hành" value={`${day.nap_am} (${day.ngu_hanh})`} />
        <Row label="Trực" value={day.truc} />
        <Row
          label="Sao (28 tú)"
          value={
            <>
              {day.tu} <span className={`tag ${day.tu_good ? 'good' : 'bad'}`}>{day.tu_good ? 'tốt' : 'xấu'}</span>
            </>
          }
        />
        <Row
          label="Hướng xuất hành"
          value={`Hỷ Thần: ${day.directions.hy_than} · Tài Thần: ${day.directions.tai_than}`}
        />
        <Row label={`Xuất hành: ${day.xuat_hanh}`} value={day.xuat_hanh_detail} />
        {day.warnings.length > 0 && <Row label="Ngày kỵ" value={day.warnings.join(', ')} />}
      </div>

      <div className="section-title">🕐 Giờ Hoàng Đạo</div>
      <div className="hours">
        {day.hours.map((h) => (
          <div key={h.chi} className={`hour ${h.good ? 'hd' : ''}`}>
            <div className="chi">
              {h.good ? '✔ ' : ''}
              {h.chi}
            </div>
            <div className="rng">{h.range}</div>
          </div>
        ))}
      </div>
    </div>
  )
}

function Row({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="row">
      <div className="label">{label}</div>
      <div className="val">{value}</div>
    </div>
  )
}

function MonthGrid({
  data,
  selected,
  today,
  onPick,
  onPrev,
  onNext,
  title,
}: {
  data: MonthData
  selected: string
  today: string
  onPick: (isoDate: string) => void
  onPrev: () => void
  onNext: () => void
  title: string
}) {
  const blanks = Array.from({ length: data.firstWeekday }, (_, i) => i)
  return (
    <div className="month-card">
      <div className="month-head">
        <button className="nav" onClick={onPrev}>
          ‹
        </button>
        <span>{title}</span>
        <button className="nav" onClick={onNext}>
          ›
        </button>
      </div>
      <div className="legend">
        <span>
          <b className="dotr">●</b> Ngày tốt (Hoàng Đạo)
        </span>
        <span>
          <b className="dotp">●</b> Ngày xấu (Hắc Đạo phạm kỵ)
        </span>
      </div>
      <div className="grid">
        {DOW_SHORT.map((d, i) => (
          <div key={d} className={`dow ${i === 6 ? 'sun' : ''}`}>
            {d}
          </div>
        ))}
        {blanks.map((b) => (
          <div key={`b${b}`} className="cell blank" />
        ))}
        {data.days.map((c) => {
          const isoDate = iso(data.year, data.month, c.solarDay)
          const dow = new Date(data.year, data.month - 1, c.solarDay).getDay() // 0=Sun
          return (
            <div
              key={c.solarDay}
              className={
                'cell' +
                (dow === 0 ? ' sunday' : '') +
                (isoDate === selected ? ' sel' : '') +
                (isoDate === today && isoDate !== selected ? ' today' : '')
              }
              onClick={() => onPick(isoDate)}
              title={`${c.dayCanChi} · Âm ${c.lunarDay}/${c.lunarMonth}${c.warnings.length ? ' · ' + c.warnings.join(', ') : ''}`}
            >
              <span className={`vdot ${c.verdict}`} />
              <div className="sol">{c.solarDay}</div>
              <div className={`lun ${c.isLunarMonthStart ? 'start' : ''}`}>
                {c.isLunarMonthStart ? `${c.lunarDay}/${c.lunarMonth}` : c.lunarDay}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function LunarLookup({ onResult }: { onResult: (isoDate: string) => void }) {
  const [ld, setLd] = useState(1)
  const [lm, setLm] = useState(1)
  const [ly, setLy] = useState(new Date().getFullYear())
  const [leap, setLeap] = useState(false)
  const [out, setOut] = useState('')
  const [err, setErr] = useState('')

  async function run() {
    setErr('')
    setOut('')
    try {
      const r = await api.lunarToSolar(ld, lm, ly, leap)
      const s = r.solar
      setOut(
        `Âm lịch ${ld}/${lm}${leap ? ' (nhuận)' : ''} năm ${r.info.year_can_chi} → Dương lịch ${iso(
          s.year,
          s.month,
          s.day,
        )} (${r.info.weekday}), ngày ${r.info.day_can_chi}.`,
      )
      onResult(iso(s.year, s.month, s.day))
    } catch (e) {
      setErr(String(e))
    }
  }

  return (
    <div className="tool">
      <h3>🔎 Đổi ngày Âm → Dương</h3>
      <div className="inline">
        <label className="muted">Ngày</label>
        <input type="number" min={1} max={30} value={ld} onChange={(e) => setLd(+e.target.value)} />
        <label className="muted">Tháng</label>
        <input type="number" min={1} max={12} value={lm} onChange={(e) => setLm(+e.target.value)} />
        <label className="muted">Năm</label>
        <input type="number" value={ly} onChange={(e) => setLy(+e.target.value)} />
        <label className="muted">
          <input type="checkbox" checked={leap} onChange={(e) => setLeap(e.target.checked)} /> nhuận
        </label>
        <button className="btn primary" onClick={run}>
          Tra cứu
        </button>
      </div>
      {out && <div className="result">{out}</div>}
      {err && <div className="err">{err}</div>}
    </div>
  )
}

function AdviseTool({ date }: { date: string }) {
  const [activity, setActivity] = useState('')
  const [out, setOut] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  async function run() {
    if (!activity.trim()) return
    setBusy(true)
    setErr('')
    setOut('')
    try {
      const r = await api.advise(date, activity.trim())
      setOut(r.text)
    } catch (e) {
      setErr('Cần bật LLM trong daemon SenClaw để dùng luận giải AI. ' + String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="tool">
      <h3>✨ Luận giải AI — ngày này hợp việc gì?</h3>
      <div className="inline">
        <input
          type="text"
          placeholder="vd: cưới hỏi, khai trương, xuất hành…"
          value={activity}
          onChange={(e) => setActivity(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && run()}
        />
        <button className="btn primary" onClick={run} disabled={busy}>
          {busy ? 'Đang luận…' : 'Xem'}
        </button>
      </div>
      <div className="muted" style={{ fontSize: 12, marginTop: 6 }}>
        Dựa trên can chi, hoàng đạo, giờ tốt và hướng của ngày {date}.
      </div>
      {out && <div className="result">{out}</div>}
      {err && <div className="err">{err}</div>}
    </div>
  )
}
