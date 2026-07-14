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

/** Relative time from an epoch value that may be seconds or ms. */
function relTime(value: number): string {
  const ms = value < 1e12 ? value * 1000 : value
  let diff = Math.max(0, Date.now() - ms)
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return `${sec}s`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h`
  const day = Math.floor(hr / 24)
  if (day < 7) return `${day}d`
  const wk = Math.floor(day / 7)
  if (wk < 5) return `${wk}w`
  const mo = Math.floor(day / 30)
  if (mo < 12) return `${mo}mo`
  return `${Math.floor(day / 365)}y`
}

export function MindmapRecentWidget() {
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

  const loading = maps === null && !err
  const recent = (maps ?? []).slice(0, 5)

  return (
    <div
      style={{
        height: '100vh',
        display: 'flex',
        flexDirection: 'column',
        padding: 14,
        background: 'var(--bg-card)',
        color: 'var(--text)',
        borderRadius: 20,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: 'var(--text-secondary)',
          display: 'flex',
          alignItems: 'center',
          gap: 5,
          marginBottom: 10,
          flexShrink: 0,
        }}
      >
        <span aria-hidden>🧠</span>
        <span>Sơ đồ gần đây</span>
      </div>

      {loading ? (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
          Đang tải…
        </div>
      ) : recent.length === 0 ? (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', fontSize: 13 }}>
          Chưa có sơ đồ nào
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2, flex: 1, minHeight: 0, overflow: 'hidden' }}>
          {recent.map((m) => (
            <div
              key={m.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                padding: '7px 2px',
                borderBottom: '1px solid var(--border)',
              }}
            >
              <div
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontSize: 14,
                  fontWeight: 600,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
                title={m.title}
              >
                {m.title || 'Không có tiêu đề'}
              </div>
              <div
                style={{
                  flexShrink: 0,
                  fontSize: 12,
                  color: 'var(--text-secondary)',
                  fontVariantNumeric: 'tabular-nums',
                  textAlign: 'right',
                }}
              >
                {(m.node_count || 0).toLocaleString('vi-VN')} nút
              </div>
              <div
                style={{
                  flexShrink: 0,
                  fontSize: 12,
                  color: 'var(--text-secondary)',
                  fontVariantNumeric: 'tabular-nums',
                  minWidth: 30,
                  textAlign: 'right',
                }}
              >
                {relTime(m.updated_at)}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
