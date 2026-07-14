import { useEffect, useState } from 'react'

const REFRESH_MS = 60000

interface DayInfo {
  solar_date: string
  weekday: string
  lunar_date: string
  hoang_dao: boolean
}

// "2026-07-14" → "14/07/2026"
function fmtSolar(iso: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso)
  if (!m) return iso
  return `${m[3]}/${m[2]}/${m[1]}`
}

export function LunaTodayWidget() {
  const [day, setDay] = useState<DayInfo | null>(null)
  const [err, setErr] = useState(false)

  useEffect(() => {
    let alive = true
    const load = () => {
      fetch('/api/day')
        .then((r) => {
          if (!r.ok) throw new Error(`HTTP ${r.status}`)
          return r.json()
        })
        .then((d: DayInfo) => {
          if (alive) {
            setDay(d)
            setErr(false)
          }
        })
        .catch(() => {
          if (alive) setErr(true)
        })
    }
    load()
    const id = setInterval(load, REFRESH_MS)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [])

  const wrap: React.CSSProperties = {
    height: '100vh',
    padding: 14,
    display: 'flex',
    flexDirection: 'column',
    justifyContent: 'center',
    alignItems: 'center',
    gap: 6,
    background: 'var(--bg-card)',
    borderRadius: 18,
    textAlign: 'center',
  }

  if (!day) {
    return (
      <div style={wrap}>
        <div style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
          {err ? 'Không tải được lịch' : 'Đang tải…'}
        </div>
      </div>
    )
  }

  const good = day.hoang_dao
  return (
    <div style={wrap}>
      <div
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: 'var(--text-secondary)',
          letterSpacing: 0.3,
        }}
      >
        {day.weekday}
      </div>
      <div
        style={{
          fontSize: 30,
          fontWeight: 800,
          lineHeight: 1.05,
          fontVariantNumeric: 'tabular-nums',
          letterSpacing: -0.5,
          whiteSpace: 'nowrap',
          color: 'var(--text)',
        }}
      >
        {fmtSolar(day.solar_date)}
      </div>
      <div style={{ fontSize: 14, color: 'var(--text-secondary)', fontVariantNumeric: 'tabular-nums' }}>
        Âm lịch {day.lunar_date}
      </div>
      <div
        style={{
          marginTop: 4,
          padding: '4px 12px',
          borderRadius: 999,
          fontSize: 12.5,
          fontWeight: 700,
          color: '#fff',
          background: good ? 'var(--success)' : 'var(--danger)',
        }}
      >
        {good ? 'Hoàng Đạo' : 'Hắc Đạo'}
      </div>
    </div>
  )
}
