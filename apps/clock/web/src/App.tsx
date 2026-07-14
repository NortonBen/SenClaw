import { useState, useEffect, useCallback, useRef } from 'react'

interface ZoneTime {
  zone: string
  label: string
  time: string
  date: string
  offset: string
}

type Tab = 'clock' | 'world' | 'timer' | 'stopwatch'

const DEFAULT_ZONES = 'Asia/Ho_Chi_Minh,America/New_York,Europe/London,Asia/Tokyo'

/// Resolve theme CSS vars to CONCRETE colors — WebKit/WKWebView (the desktop
/// webview) doesn't accept `var(--x)` inside SVG fill/stroke attributes.
function themeColors() {
  const fb = { face: '#f5f5f5', border: '#e5e7eb', text: '#1a1a2e', secondary: '#6b7280', accent: '#3b82f6' }
  if (typeof window === 'undefined') return fb
  const cs = getComputedStyle(document.documentElement)
  const g = (v: string, f: string) => cs.getPropertyValue(v).trim() || f
  return {
    face: g('--bg-card', fb.face),
    border: g('--border', fb.border),
    text: g('--text', fb.text),
    secondary: g('--text-secondary', fb.secondary),
    accent: g('--accent', fb.accent),
  }
}

function useThemeColors() {
  const [col, setCol] = useState(themeColors)
  useEffect(() => {
    setCol(themeColors())
    const obs = new MutationObserver(() => setCol(themeColors()))
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => obs.disconnect()
  }, [])
  return col
}

function AnalogClock({ size = 220 }: { size?: number }) {
  const [now, setNow] = useState(new Date())
  const col = useThemeColors()

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(id)
  }, [])

  const cx = size / 2
  const cy = size / 2
  const r = size / 2 - 10

  const sec = now.getSeconds()
  const min = now.getMinutes()
  const hr = now.getHours() % 12

  const secAngle = (sec / 60) * 360 - 90
  const minAngle = ((min + sec / 60) / 60) * 360 - 90
  const hrAngle = ((hr + min / 60) / 12) * 360 - 90

  const hand = (angle: number, len: number, width: number, color: string) => {
    const rad = (angle * Math.PI) / 180
    const x2 = cx + len * Math.cos(rad)
    const y2 = cy + len * Math.sin(rad)
    return <line x1={cx} y1={cy} x2={x2} y2={y2} stroke={color} strokeWidth={width} strokeLinecap="round" />
  }

  const markers = Array.from({ length: 12 }, (_, i) => {
    const angle = ((i / 12) * 360 - 90) * Math.PI / 180
    const inner = r - (i % 3 === 0 ? 18 : 10)
    const outer = r - 2
    return (
      <line
        key={i}
        x1={cx + inner * Math.cos(angle)}
        y1={cy + inner * Math.sin(angle)}
        x2={cx + outer * Math.cos(angle)}
        y2={cy + outer * Math.sin(angle)}
        stroke={col.secondary}
        strokeWidth={i % 3 === 0 ? 3 : 1.5}
        strokeLinecap="round"
      />
    )
  })

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle cx={cx} cy={cy} r={r} fill={col.face} stroke={col.border} strokeWidth={2} />
        {markers}
        {hand(hrAngle, r * 0.5, 5, col.text)}
        {hand(minAngle, r * 0.7, 3, col.text)}
        {hand(secAngle, r * 0.8, 1.5, col.accent)}
        <circle cx={cx} cy={cy} r={4} fill={col.accent} />
      </svg>
      <div style={{ textAlign: 'center' }}>
        <div style={{ fontSize: 32, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>
          {now.toLocaleTimeString('vi-VN', { hour12: false })}
        </div>
        <div style={{ fontSize: 14, color: 'var(--text-secondary)' }}>
          {now.toLocaleDateString('vi-VN', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' })}
        </div>
      </div>
    </div>
  )
}

function WorldClock() {
  const [zones, setZones] = useState<ZoneTime[]>([])

  const fetchTime = useCallback(async () => {
    try {
      const res = await fetch(`/api/time?zones=${DEFAULT_ZONES}`)
      if (res.ok) {
        const data = await res.json()
        setZones(data.zones)
        return
      }
    } catch { /* fall through to client-side */ }
    const now = new Date()
    const fallback = DEFAULT_ZONES.split(',').map(z => {
      const tz = z.trim()
      const fmt = new Intl.DateTimeFormat('en-GB', { timeZone: tz, hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
      const dfmt = new Intl.DateTimeFormat('en-CA', { timeZone: tz, year: 'numeric', month: '2-digit', day: '2-digit' })
      return { zone: tz, label: tz.split('/').pop()!.replace(/_/g, ' '), time: fmt.format(now), date: dfmt.format(now), offset: '' }
    })
    setZones(fallback)
  }, [])

  useEffect(() => {
    fetchTime()
    const id = setInterval(fetchTime, 1000)
    return () => clearInterval(id)
  }, [fetchTime])

  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 12 }}>
      {zones.map(z => (
        <div key={z.zone} style={{
          background: 'var(--bg-card)', borderRadius: 12, padding: 16,
          boxShadow: 'var(--shadow)', border: '1px solid var(--border)',
        }}>
          <div style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 4 }}>{z.label}</div>
          <div style={{ fontSize: 28, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{z.time}</div>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>{z.date} {z.offset}</div>
        </div>
      ))}
    </div>
  )
}

function Timer() {
  const [totalSec, setTotalSec] = useState(300)
  const [remaining, setRemaining] = useState(300)
  const [running, setRunning] = useState(false)
  const col = useThemeColors()
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  useEffect(() => {
    if (running && remaining > 0) {
      intervalRef.current = setInterval(() => setRemaining(r => Math.max(0, r - 1)), 1000)
    }
    return () => { if (intervalRef.current) clearInterval(intervalRef.current) }
  }, [running, remaining])

  useEffect(() => {
    if (remaining === 0 && running) setRunning(false)
  }, [remaining, running])

  const mm = Math.floor(remaining / 60)
  const ss = remaining % 60
  const pct = totalSec > 0 ? remaining / totalSec : 0

  const presets = [60, 180, 300, 600, 900, 1800]

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 24 }}>
      <div style={{ position: 'relative', width: 200, height: 200 }}>
        <svg width={200} height={200} viewBox="0 0 200 200">
          <circle cx={100} cy={100} r={88} fill="none" stroke={col.border} strokeWidth={6} />
          <circle cx={100} cy={100} r={88} fill="none" stroke={col.accent} strokeWidth={6}
            strokeDasharray={2 * Math.PI * 88}
            strokeDashoffset={2 * Math.PI * 88 * (1 - pct)}
            strokeLinecap="round"
            style={{ transform: 'rotate(-90deg)', transformOrigin: 'center', transition: 'stroke-dashoffset 0.3s' }}
          />
        </svg>
        <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <span style={{ fontSize: 36, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>
            {String(mm).padStart(2, '0')}:{String(ss).padStart(2, '0')}
          </span>
        </div>
      </div>
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', justifyContent: 'center' }}>
        {presets.map(p => (
          <button key={p} onClick={() => { setTotalSec(p); setRemaining(p); setRunning(false) }}
            style={{
              padding: '6px 14px', borderRadius: 8, border: '1px solid var(--border)',
              background: totalSec === p ? 'var(--accent)' : 'var(--bg-card)',
              color: totalSec === p ? '#fff' : 'var(--text)', cursor: 'pointer', fontSize: 13,
            }}>
            {p >= 60 ? `${p / 60}m` : `${p}s`}
          </button>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 12 }}>
        <button onClick={() => setRunning(!running)} style={{
          padding: '10px 28px', borderRadius: 10, border: 'none', fontSize: 15, fontWeight: 600,
          background: running ? '#ef4444' : 'var(--accent)', color: '#fff', cursor: 'pointer',
        }}>
          {running ? 'Dừng' : 'Bắt đầu'}
        </button>
        <button onClick={() => { setRunning(false); setRemaining(totalSec) }} style={{
          padding: '10px 28px', borderRadius: 10, border: '1px solid var(--border)',
          background: 'var(--bg-card)', color: 'var(--text)', cursor: 'pointer', fontSize: 15,
        }}>
          Đặt lại
        </button>
      </div>
    </div>
  )
}

function Stopwatch() {
  const [elapsed, setElapsed] = useState(0)
  const [running, setRunning] = useState(false)
  const [laps, setLaps] = useState<number[]>([])
  const startRef = useRef<number>(0)
  const rafRef = useRef<number>(0)

  useEffect(() => {
    if (running) {
      startRef.current = performance.now() - elapsed
      const tick = () => {
        setElapsed(performance.now() - startRef.current)
        rafRef.current = requestAnimationFrame(tick)
      }
      rafRef.current = requestAnimationFrame(tick)
    }
    return () => cancelAnimationFrame(rafRef.current)
  }, [running])

  const fmt = (ms: number) => {
    const m = Math.floor(ms / 60000)
    const s = Math.floor((ms % 60000) / 1000)
    const cs = Math.floor((ms % 1000) / 10)
    return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}.${String(cs).padStart(2, '0')}`
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 24 }}>
      <div style={{ fontSize: 48, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{fmt(elapsed)}</div>
      <div style={{ display: 'flex', gap: 12 }}>
        <button onClick={() => setRunning(!running)} style={{
          padding: '10px 28px', borderRadius: 10, border: 'none', fontSize: 15, fontWeight: 600,
          background: running ? '#ef4444' : 'var(--accent)', color: '#fff', cursor: 'pointer',
        }}>
          {running ? 'Dừng' : elapsed > 0 ? 'Tiếp tục' : 'Bắt đầu'}
        </button>
        {running && (
          <button onClick={() => setLaps(prev => [elapsed, ...prev])} style={{
            padding: '10px 28px', borderRadius: 10, border: '1px solid var(--border)',
            background: 'var(--bg-card)', color: 'var(--text)', cursor: 'pointer', fontSize: 15,
          }}>
            Vòng
          </button>
        )}
        {!running && elapsed > 0 && (
          <button onClick={() => { setElapsed(0); setLaps([]) }} style={{
            padding: '10px 28px', borderRadius: 10, border: '1px solid var(--border)',
            background: 'var(--bg-card)', color: 'var(--text)', cursor: 'pointer', fontSize: 15,
          }}>
            Đặt lại
          </button>
        )}
      </div>
      {laps.length > 0 && (
        <div style={{ width: '100%', maxWidth: 300 }}>
          {laps.map((lap, i) => (
            <div key={i} style={{
              display: 'flex', justifyContent: 'space-between', padding: '8px 0',
              borderBottom: '1px solid var(--border)', fontSize: 14,
            }}>
              <span style={{ color: 'var(--text-secondary)' }}>Vòng {laps.length - i}</span>
              <span style={{ fontVariantNumeric: 'tabular-nums' }}>{fmt(lap)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

const TABS: { key: Tab; label: string; icon: string }[] = [
  { key: 'clock', label: 'Đồng hồ', icon: '🕐' },
  { key: 'world', label: 'Thế giới', icon: '🌍' },
  { key: 'timer', label: 'Hẹn giờ', icon: '⏱️' },
  { key: 'stopwatch', label: 'Bấm giờ', icon: '⏲️' },
]

export default function App() {
  const [tab, setTab] = useState<Tab>('clock')

  useEffect(() => {
    const handleMessage = (e: MessageEvent) => {
      if (e.data?.type === 'senclaw:init' || e.data?.type === 'senclaw:theme') {
        const t = e.data.theme || e.data.env?.theme
        if (t) document.documentElement.setAttribute('data-theme', t)
      }
    }
    window.addEventListener('message', handleMessage)
    window.parent.postMessage({ type: 'senclaw:ready' }, '*')
    return () => window.removeEventListener('message', handleMessage)
  }, [])

  return (
    <div style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column' }}>
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 24 }}>
        {tab === 'clock' && <AnalogClock />}
        {tab === 'world' && <WorldClock />}
        {tab === 'timer' && <Timer />}
        {tab === 'stopwatch' && <Stopwatch />}
      </div>
      <nav style={{
        display: 'flex', justifyContent: 'center', gap: 0,
        borderTop: '1px solid var(--border)', background: 'var(--bg-card)',
        padding: '8px 0', position: 'sticky', bottom: 0,
      }}>
        {TABS.map(t => (
          <button key={t.key} onClick={() => setTab(t.key)} style={{
            display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2,
            padding: '6px 20px', border: 'none', background: 'transparent', cursor: 'pointer',
            color: tab === t.key ? 'var(--accent)' : 'var(--text-secondary)', fontSize: 11, fontWeight: 500,
          }}>
            <span style={{ fontSize: 20 }}>{t.icon}</span>
            {t.label}
          </button>
        ))}
      </nav>
    </div>
  )
}
