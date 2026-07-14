import { useEffect, useState } from 'react'

const REFRESH_MS = 60000

interface Stats {
  customers?: number
  total?: number
  open_deals?: number
  open_tasks?: number
  overdue_tasks?: number
}

interface Tile {
  label: string
  value: number
  danger?: boolean
}

const numFmt: React.CSSProperties = { fontVariantNumeric: 'tabular-nums' }

export function CrmOverviewWidget() {
  const [stats, setStats] = useState<Stats | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let alive = true
    const load = async () => {
      try {
        const r = await fetch('/api/stats')
        if (!r.ok) throw new Error(String(r.status))
        const d: Stats = await r.json()
        if (alive) { setStats(d); setError(false) }
      } catch {
        if (alive) setError(true)
      }
    }
    load()
    const id = setInterval(load, REFRESH_MS)
    return () => { alive = false; clearInterval(id) }
  }, [])

  const wrap: React.CSSProperties = {
    height: '100vh',
    padding: 14,
    display: 'flex',
    flexDirection: 'column',
  }

  if (!stats && !error) {
    return (
      <div style={{ ...wrap, alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
        Đang tải…
      </div>
    )
  }
  if (error && !stats) {
    return (
      <div style={{ ...wrap, alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
        Không tải được dữ liệu
      </div>
    )
  }

  const s = stats!
  const overdue = s.overdue_tasks ?? 0
  const tiles: Tile[] = [
    { label: 'Khách hàng', value: s.customers ?? s.total ?? 0 },
    { label: 'Deals mở', value: s.open_deals ?? 0 },
    { label: 'Việc cần làm', value: s.open_tasks ?? 0 },
    { label: 'Quá hạn', value: overdue, danger: overdue > 0 },
  ]

  return (
    <div style={wrap}>
      <div style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', color: 'var(--text-secondary)', marginBottom: 10 }}>
        Thông tin trên CRM
      </div>
      <div
        style={{
          flex: 1,
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gridTemplateRows: '1fr 1fr',
          gap: 10,
        }}
      >
        {tiles.map((t) => (
          <div
            key={t.label}
            style={{
              background: 'var(--bg-card)',
              border: '1px solid var(--border)',
              borderRadius: 16,
              padding: '12px 14px',
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'center',
              minWidth: 0,
            }}
          >
            <div
              style={{
                ...numFmt,
                fontSize: 30,
                lineHeight: 1,
                fontWeight: 700,
                color: t.danger ? 'var(--danger)' : 'var(--text)',
              }}
            >
              {t.value.toLocaleString('en-US')}
            </div>
            <div
              style={{
                fontSize: 12,
                marginTop: 6,
                color: t.danger ? 'var(--danger)' : 'var(--text-secondary)',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
              }}
            >
              {t.label}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
