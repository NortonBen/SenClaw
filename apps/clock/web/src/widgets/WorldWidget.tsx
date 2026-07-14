import { useState, useEffect } from 'react'

const ZONES = ['Asia/Ho_Chi_Minh', 'America/New_York', 'Europe/London', 'Asia/Tokyo']
const LABELS: Record<string, string> = {
  'Asia/Ho_Chi_Minh': 'HN',
  'America/New_York': 'NY',
  'Europe/London': 'LDN',
  'Asia/Tokyo': 'TKY',
}

export function WorldWidget() {
  const [times, setTimes] = useState<{ zone: string; time: string }[]>([])

  useEffect(() => {
    const update = () => {
      const now = new Date()
      setTimes(ZONES.map(z => {
        const fmt = new Intl.DateTimeFormat('en-GB', {
          timeZone: z, hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
        })
        return { zone: z, time: fmt.format(now) }
      }))
    }
    update()
    const id = setInterval(update, 1000)
    return () => clearInterval(id)
  }, [])

  return (
    <div style={{
      display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, padding: 12,
      height: '100%', alignContent: 'center',
    }}>
      {times.map(t => (
        <div key={t.zone} style={{
          background: 'var(--bg-card)', borderRadius: 10, padding: '8px 10px',
          border: '1px solid var(--border)',
        }}>
          <div style={{ fontSize: 10, color: 'var(--text-secondary)' }}>{LABELS[t.zone] ?? t.zone}</div>
          <div style={{ fontSize: 18, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{t.time}</div>
        </div>
      ))}
    </div>
  )
}
