import { useEffect, useState } from 'react'

interface MapMeta {
  id: number
  title: string
  description: string | null
  layout: string
  created_at: number
  updated_at: number
  node_count: number
}

const REFRESH_MS = 60000

export function MindmapCountWidget() {
  const [maps, setMaps] = useState<MapMeta[] | null>(null)
  const [err, setErr] = useState(false)

  useEffect(() => {
    let alive = true
    const load = async () => {
      try {
        const res = await fetch('/api/maps')
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        const data = (await res.json()) as MapMeta[]
        if (alive) { setMaps(Array.isArray(data) ? data : []); setErr(false) }
      } catch {
        if (alive) setErr(true)
      }
    }
    load()
    const t = setInterval(load, REFRESH_MS)
    return () => { alive = false; clearInterval(t) }
  }, [])

  const count = maps?.length ?? 0
  const nodes = maps?.reduce((s, m) => s + (m.node_count || 0), 0) ?? 0
  const loading = maps === null && !err

  return (
    <div
      style={{
        height: '100vh',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 4,
        padding: 14,
        background: 'var(--bg-card)',
        color: 'var(--text)',
        borderRadius: 20,
        textAlign: 'center',
      }}
    >
      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-secondary)', display: 'flex', alignItems: 'center', gap: 5 }}>
        <span aria-hidden>🧠</span>
        <span>Sơ đồ</span>
      </div>
      <div
        style={{
          fontSize: 46,
          lineHeight: 1.05,
          fontWeight: 700,
          color: 'var(--accent)',
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {loading ? '—' : err ? '!' : count}
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-secondary)', fontVariantNumeric: 'tabular-nums' }}>
        {loading || err ? ' ' : `${nodes.toLocaleString('vi-VN')} nút`}
      </div>
    </div>
  )
}
