import { useState, useEffect } from 'react'

/// Resolve the CSS theme vars to CONCRETE colors. SVG presentation attributes
/// (fill/stroke) don't reliably accept `var(--x)` in WebKit/WKWebView (the
/// macOS/iOS desktop webview) — so we read the computed values and pass hex.
function themeColors() {
  const fb = {
    face: '#f5f5f5',
    border: '#e5e7eb',
    text: '#1a1a2e',
    secondary: '#6b7280',
    accent: '#3b82f6',
  }
  if (typeof window === 'undefined') return fb
  const cs = getComputedStyle(document.documentElement)
  const g = (v: string, f: string) => {
    const x = cs.getPropertyValue(v).trim()
    return x || f
  }
  return {
    face: g('--bg-card', fb.face),
    border: g('--border', fb.border),
    text: g('--text', fb.text),
    secondary: g('--text-secondary', fb.secondary),
    accent: g('--accent', fb.accent),
  }
}

export function AnalogWidget() {
  const [now, setNow] = useState(new Date())
  const [col, setCol] = useState(themeColors)

  useEffect(() => {
    const tick = () => {
      setNow(new Date())
      setCol(themeColors())
    }
    tick()
    const id = setInterval(tick, 1000)
    // Re-read colors when the host pushes a theme change (data-theme attr).
    const obs = new MutationObserver(() => setCol(themeColors()))
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })
    return () => {
      clearInterval(id)
      obs.disconnect()
    }
  }, [])

  const size = 160
  const cx = size / 2
  const cy = size / 2
  const r = size / 2 - 8

  const sec = now.getSeconds()
  const min = now.getMinutes()
  const hr = now.getHours() % 12

  const hand = (angle: number, len: number, width: number, color: string) => {
    const rad = (angle * Math.PI) / 180
    return <line x1={cx} y1={cy} x2={cx + len * Math.cos(rad)} y2={cy + len * Math.sin(rad)}
      stroke={color} strokeWidth={width} strokeLinecap="round" />
  }

  const markers = Array.from({ length: 12 }, (_, i) => {
    const a = ((i / 12) * 360 - 90) * Math.PI / 180
    return <line key={i}
      x1={cx + (r - (i % 3 === 0 ? 14 : 8)) * Math.cos(a)} y1={cy + (r - (i % 3 === 0 ? 14 : 8)) * Math.sin(a)}
      x2={cx + (r - 2) * Math.cos(a)} y2={cy + (r - 2) * Math.sin(a)}
      stroke={col.secondary} strokeWidth={i % 3 === 0 ? 2.5 : 1} strokeLinecap="round" />
  })

  const secA = (sec / 60) * 360 - 90
  const minA = ((min + sec / 60) / 60) * 360 - 90
  const hrA = ((hr + min / 60) / 12) * 360 - 90

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', gap: 4 }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle cx={cx} cy={cy} r={r} fill={col.face} stroke={col.border} strokeWidth={1.5} />
        {markers}
        {hand(hrA, r * 0.5, 4, col.text)}
        {hand(minA, r * 0.7, 2.5, col.text)}
        {hand(secA, r * 0.8, 1, col.accent)}
        <circle cx={cx} cy={cy} r={3} fill={col.accent} />
      </svg>
    </div>
  )
}
