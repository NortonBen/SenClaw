import { useEffect, useState } from 'react'

const REFRESH_MS = 60000

interface Stats {
  open_deals?: number
  pipeline_value?: number
}

const numFmt: React.CSSProperties = { fontVariantNumeric: 'tabular-nums' }

export function CrmPipelineWidget() {
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
    justifyContent: 'center',
  }

  if (!stats && !error) {
    return (
      <div style={{ ...wrap, alignItems: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
        Đang tải…
      </div>
    )
  }
  if (error && !stats) {
    return (
      <div style={{ ...wrap, alignItems: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
        Không tải được dữ liệu
      </div>
    )
  }

  const s = stats!
  const openDeals = s.open_deals ?? 0
  const pipeline = Math.round(s.pipeline_value ?? 0)

  return (
    <div style={wrap}>
      <div style={{ fontSize: 11, fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', color: 'var(--text-secondary)', marginBottom: 8 }}>
        Pipeline
      </div>
      <div style={{ ...numFmt, fontSize: 46, lineHeight: 1, fontWeight: 700, color: 'var(--accent)' }}>
        {openDeals.toLocaleString('en-US')}
      </div>
      <div style={{ fontSize: 13, marginTop: 6, color: 'var(--text-secondary)' }}>
        Deals đang mở
      </div>
      <div style={{ ...numFmt, fontSize: 13, marginTop: 12, color: 'var(--text)', fontWeight: 600 }}>
        {pipeline.toLocaleString('en-US')} ₫
      </div>
    </div>
  )
}
