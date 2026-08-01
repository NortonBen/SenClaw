import { useMemo, useRef, useState } from 'react'
import { Typography } from 'antd'
import type { Resolved } from './theme'

/**
 * A small area chart for one measure over time.
 *
 * CPU (%) and RAM (MB) get **one chart each** rather than two lines on one
 * plot: they are different units on different scales, and a dual-axis chart
 * lets the reader infer a relationship from where the lines happen to cross,
 * which is an artefact of the two scales rather than anything in the data.
 *
 * Text lives in HTML around the SVG, never inside it. The plot stretches with
 * `preserveAspectRatio="none"` so it always fills its column, and anything
 * drawn inside would stretch with it — the stroke is exempted explicitly via
 * `vector-effect`, but glyphs cannot be.
 */

/** Series colours, stepped per surface. Validated for both modes. */
export const SERIES = {
  cpu: { light: '#2a78d6', dark: '#3987e5' },
  ram: { light: '#eb6834', dark: '#d95926' },
} as const

const W = 300
const H = 72
/** Vertical space reserved above the plot for the hover tooltip. */
const TOOLTIP_H = 24

/** Keep the tooltip inside the card at both ends of the series. */
function clampPct(p: number): number {
  return Math.min(94, Math.max(6, p))
}

/**
 * How much of its own width the tooltip shifts left. Centred in the middle of
 * the chart, left-aligned near the start, right-aligned near the end — so it
 * stays over the plot instead of centring past its edge.
 */
function anchorPct(p: number): number {
  if (p < 12) return 0
  if (p > 88) return 100
  return 50
}

export function AreaSpark({
  points,
  color,
  ceiling,
  format,
  mode,
  sampleMs,
}: {
  points: number[]
  color: string
  /** Top of the y scale. Fixed by the caller so the axis does not rescale on every tick. */
  ceiling: number
  format: (v: number) => string
  mode: Resolved
  sampleMs: number
}) {
  const host = useRef<HTMLDivElement>(null)
  const [hover, setHover] = useState<number | null>(null)

  const grid = mode === 'dark' ? 'rgba(255,255,255,0.10)' : 'rgba(0,0,0,0.08)'
  const gid = useMemo(() => `g${Math.abs(hashCode(color))}`, [color])

  const n = points.length
  const scaleY = (v: number) => H - (Math.min(v, ceiling) / ceiling) * H
  const scaleX = (i: number) => (n <= 1 ? 0 : (i / (n - 1)) * W)

  const line = points.map((v, i) => `${scaleX(i)},${scaleY(v)}`).join(' ')
  // The fill is the line closed down to the baseline.
  const area = n > 1 ? `${scaleX(0)},${H} ${line} ${scaleX(n - 1)},${H}` : ''

  const onMove = (e: React.MouseEvent) => {
    if (n < 2 || !host.current) return
    const r = host.current.getBoundingClientRect()
    const frac = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width))
    setHover(Math.round(frac * (n - 1)))
  }

  const hoveredValue = hover != null ? points[hover] : null
  // Seconds before "now" for the hovered sample.
  const hoveredAgo =
    hover != null ? Math.round(((n - 1 - hover) * sampleMs) / 1000) : 0

  return (
    <div
      ref={host}
      // The tooltip sits above the plot, so its height is reserved here. Without
      // the reservation it renders on top of the header line and covers the
      // current value it is meant to complement.
      style={{ position: 'relative', width: '100%', paddingTop: TOOLTIP_H }}
      onMouseMove={onMove}
      onMouseLeave={() => setHover(null)}
    >
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        style={{ width: '100%', height: H, display: 'block' }}
        role="img"
        aria-label={`Biểu đồ theo thời gian, giá trị hiện tại ${format(points[n - 1] ?? 0)}`}
      >
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity="0.34" />
            <stop offset="100%" stopColor={color} stopOpacity="0.02" />
          </linearGradient>
        </defs>

        {/* Recessive grid: baseline, midpoint, top of scale. */}
        {[0, 0.5, 1].map((f) => (
          <line
            key={f}
            x1="0"
            x2={W}
            y1={H * f}
            y2={H * f}
            stroke={grid}
            strokeWidth="1"
            vectorEffect="non-scaling-stroke"
          />
        ))}

        {n > 1 && (
          <>
            <polygon points={area} fill={`url(#${gid})`} />
            <polyline
              points={line}
              fill="none"
              stroke={color}
              strokeWidth="2"
              strokeLinejoin="round"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
            />
          </>
        )}

        {hover != null && n > 1 && (
          <>
            <line
              x1={scaleX(hover)}
              x2={scaleX(hover)}
              y1="0"
              y2={H}
              stroke={color}
              strokeWidth="1"
              strokeDasharray="3 3"
              vectorEffect="non-scaling-stroke"
            />
            {/* The dot is drawn in a non-scaling wrapper so it stays round
                however wide the chart gets. */}
            <circle
              cx={scaleX(hover)}
              cy={scaleY(points[hover])}
              r="3"
              fill={color}
              stroke={mode === 'dark' ? '#1a1a19' : '#fcfcfb'}
              strokeWidth="2"
              vectorEffect="non-scaling-stroke"
              style={{ transformBox: 'fill-box' }}
            />
          </>
        )}
      </svg>

      {hoveredValue != null && (
        <div
          style={{
            position: 'absolute',
            top: 0,
            // Clamped away from both edges, and the anchor point shifts with it,
            // so a tooltip at the first or last sample does not hang outside
            // the card.
            left: `${clampPct((hover! / Math.max(1, n - 1)) * 100)}%`,
            transform: `translate(-${anchorPct(
              (hover! / Math.max(1, n - 1)) * 100,
            )}%, 0)`,
            pointerEvents: 'none',
            whiteSpace: 'nowrap',
            padding: '2px 7px',
            borderRadius: 6,
            fontSize: 12,
            background: mode === 'dark' ? '#262625' : '#ffffff',
            border: `1px solid ${grid}`,
            boxShadow: '0 2px 8px rgba(0,0,0,0.18)',
          }}
        >
          {format(hoveredValue)}
          <span style={{ opacity: 0.6 }}>
            {' · '}
            {hoveredAgo === 0 ? 'bây giờ' : `${hoveredAgo}s trước`}
          </span>
        </div>
      )}
    </div>
  )
}

/** Header line for a chart: what it measures, and the value right now. */
export function ChartHeader({
  title,
  value,
  color,
  ceilingLabel,
}: {
  title: string
  value: string
  color: string
  ceilingLabel: string
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'baseline',
        justifyContent: 'space-between',
        gap: 8,
      }}
    >
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        {/* The swatch carries identity; the label stays in text ink. */}
        <span
          style={{
            display: 'inline-block',
            width: 8,
            height: 8,
            borderRadius: 2,
            background: color,
            marginRight: 6,
          }}
        />
        {title}
      </Typography.Text>
      <Typography.Text strong>{value}</Typography.Text>
      <Typography.Text type="secondary" style={{ fontSize: 11 }}>
        đỉnh trục {ceilingLabel}
      </Typography.Text>
    </div>
  )
}

function hashCode(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h << 5) - h + s.charCodeAt(i)
  return h
}
