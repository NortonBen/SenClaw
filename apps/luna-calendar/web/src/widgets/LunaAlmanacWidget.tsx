import { useEffect, useState } from 'react'

const REFRESH_MS = 60000

interface DayInfo {
  lunar_date: string
  day_can_chi: string
  hoang_dao: boolean
  good_hours: string
  verdict_label: string
  warnings: string[]
}

export function LunaAlmanacWidget() {
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
    gap: 10,
    background: 'var(--bg-card)',
    borderRadius: 18,
    overflow: 'hidden',
  }

  if (!day) {
    return (
      <div style={{ ...wrap, alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
          {err ? 'Không tải được lịch' : 'Đang tải…'}
        </div>
      </div>
    )
  }

  const good = day.hoang_dao
  return (
    <div style={wrap}>
      {/* Header: lunar date + can chi, with the Hoàng/Hắc badge */}
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <div style={{ minWidth: 0, flex: 1 }}>
          <div
            style={{
              fontSize: 19,
              fontWeight: 800,
              color: 'var(--text)',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            Âm lịch {day.lunar_date}
          </div>
          <div style={{ fontSize: 13, color: 'var(--text-secondary)', marginTop: 2 }}>
            Ngày {day.day_can_chi}
          </div>
        </div>
        <div
          style={{
            flexShrink: 0,
            padding: '4px 11px',
            borderRadius: 999,
            fontSize: 12,
            fontWeight: 700,
            color: '#fff',
            background: good ? 'var(--success)' : 'var(--danger)',
          }}
        >
          {good ? 'Hoàng Đạo' : 'Hắc Đạo'}
        </div>
      </div>

      {/* Giờ hoàng đạo */}
      <div
        style={{
          fontSize: 13,
          lineHeight: 1.45,
          color: 'var(--text)',
          background: 'color-mix(in srgb, var(--accent) 12%, transparent)',
          border: '1px solid var(--border)',
          borderRadius: 12,
          padding: '8px 10px',
        }}
      >
        <span style={{ color: 'var(--text-secondary)', fontWeight: 600 }}>Giờ hoàng đạo: </span>
        <span style={{ fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>
          {day.good_hours || '—'}
        </span>
      </div>

      {/* Verdict summary */}
      {day.verdict_label && (
        <div style={{ fontSize: 13.5, fontWeight: 600, color: 'var(--text)' }}>
          {day.verdict_label}
        </div>
      )}

      {/* Warning chips */}
      {day.warnings && day.warnings.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 'auto' }}>
          {day.warnings.map((w) => (
            <span
              key={w}
              style={{
                fontSize: 11,
                fontWeight: 600,
                color: 'var(--danger)',
                background: 'color-mix(in srgb, var(--danger) 14%, transparent)',
                border: '1px solid color-mix(in srgb, var(--danger) 40%, transparent)',
                borderRadius: 999,
                padding: '2px 9px',
              }}
            >
              {w}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
