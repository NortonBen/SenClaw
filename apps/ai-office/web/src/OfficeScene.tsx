import { useEffect, useState } from 'react'
import type { Agent, OfficeEvent } from './types'

/* Isometric wireframe office, mirroring the reel: desks on a 2:1 iso grid,
   one figure per agent, status dots, walk-to-handoff animation and speech
   bubbles. Pure SVG — no canvas, no deps. */

const ISO_X = 46
const ISO_Y = 23

function iso(x: number, y: number, z = 0): [number, number] {
  return [(x - y) * ISO_X, (x + y) * ISO_Y - z]
}

function pts(list: [number, number][]): string {
  return list.map(([a, b]) => `${a.toFixed(1)},${b.toFixed(1)}`).join(' ')
}

/** Grid coordinates (office-space units) of each desk. */
const DESKS: Record<string, { x: number; y: number; label: string }> = {
  'nghien-cuu': { x: 1.6, y: 1.0, label: 'NGHIÊN CỨU' },
  'noi-dung': { x: 3.6, y: 1.0, label: 'NỘI DUNG' },
  'kiem-dinh': { x: 5.6, y: 1.0, label: 'KIỂM ĐỊNH' },
  'truong-phong': { x: 1.6, y: 3.4, label: 'TRƯỞNG PHÒNG' },
  'phan-tich': { x: 5.6, y: 3.4, label: 'PHÂN TÍCH' },
  sep: { x: 7.9, y: 2.2, label: 'SẾP (BẠN)' },
}

const STATUS_COLOR: Record<string, string> = {
  working: 'var(--working)',
  done: 'var(--done)',
  handoff: 'var(--handoff)',
  idle: 'var(--idle)',
}

/** Wireframe cuboid at grid (x,y): a desk with a small monitor. */
function Desk({ x, y }: { x: number; y: number }) {
  const w = 1.1
  const d = 0.62
  const h = 26
  const base: [number, number][] = [iso(x, y), iso(x + w, y), iso(x + w, y + d), iso(x, y + d)]
  const top: [number, number][] = base.map(([a, b]) => [a, b - h])
  const stroke = 'var(--ink)'
  const monC = iso(x + w * 0.5, y + d * 0.4)
  return (
    <g strokeWidth="1" stroke={stroke} fill="var(--panel)" fillOpacity="0.75">
      {/* legs */}
      {base.map(([a, b], i) => (
        <line key={i} x1={a} y1={b} x2={a} y2={b - h} />
      ))}
      {/* top slab */}
      <polygon points={pts(top)} />
      {/* monitor: small upright quad on the desk */}
      <polygon
        points={pts([
          [monC[0] - 9, monC[1] - h - 2],
          [monC[0] + 9, monC[1] - h - 7],
          [monC[0] + 9, monC[1] - h - 19],
          [monC[0] - 9, monC[1] - h - 14],
        ])}
        fill="var(--paper)"
      />
    </g>
  )
}

/** A little person: head + trapezoid body, tinted by status. */
function Figure({ color }: { color: string }) {
  return (
    <g>
      <ellipse cx="0" cy="-4" rx="7" ry="3.4" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.8" opacity="0.85" />
      <polygon points="-5,-5 5,-5 3.4,-22 -3.4,-22" fill="var(--panel)" stroke="var(--ink)" strokeWidth="1" />
      <circle cx="0" cy="-27" r="4.6" fill="var(--panel)" stroke="var(--ink)" strokeWidth="1" />
      <circle cx="6" cy="-24" r="3.2" fill={color} stroke="var(--ink)" strokeWidth="0.6" />
    </g>
  )
}

interface Bubble {
  actor: string
  text: string
  until: number
}

export function OfficeScene({ agents, events }: { agents: Agent[]; events: OfficeEvent[] }) {
  const [bubble, setBubble] = useState<Bubble | null>(null)

  // Latest bubble/handoff event drives a transient speech bubble (~4.5s).
  useEffect(() => {
    const last = [...events].reverse().find((e) => e.kind === 'bubble')
    if (!last) return
    const ageMs = Date.now() - last.created_at * 1000
    if (ageMs < 6000) {
      setBubble({ actor: last.actor, text: last.text, until: Date.now() + 4500 })
      const t = setTimeout(() => setBubble(null), 4500)
      return () => clearTimeout(t)
    }
  }, [events])

  // Where each agent stands: at its desk, or walking to the handoff target.
  const posOf = (a: Agent): { x: number; y: number } => {
    const home = DESKS[a.key] ?? DESKS['truong-phong']
    if (a.status === 'handoff') {
      const last = [...events]
        .reverse()
        .find((e) => (e.kind === 'bubble' || e.kind === 'handoff') && e.actor === a.key)
      const target = last && DESKS[last.target] ? DESKS[last.target] : DESKS['truong-phong']
      // Stand just beside the target desk, not on top of it.
      return { x: target.x - 0.55, y: target.y + 0.4 }
    }
    return { x: home.x + 0.5, y: home.y + 1.05 }
  }

  // Floor grid 0..9 x 0..5
  const grid: [number, number][][] = []
  for (let i = 0; i <= 9; i++) grid.push([iso(i, 0), iso(i, 5)])
  for (let j = 0; j <= 5; j++) grid.push([iso(0, j), iso(9, j)])

  // Back walls (wireframe)
  const wallH = 78
  const wallA: [number, number][] = [iso(0, 0), iso(9, 0)]
  const wallB: [number, number][] = [iso(0, 0), iso(0, 5)]

  return (
    <svg className="office" viewBox="-260 -130 780 340" role="img" aria-label="Mô phỏng văn phòng">
      <g transform="translate(0,60)">
        {/* floor */}
        <g stroke="var(--line)" strokeWidth="0.7">
          {grid.map(([a, b], i) => (
            <line key={i} x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />
          ))}
        </g>
        {/* walls */}
        <g stroke="var(--line-strong)" strokeWidth="1" fill="none">
          <polyline points={pts([[wallA[0][0], wallA[0][1] - wallH], wallA[0], wallA[1], [wallA[1][0], wallA[1][1] - wallH]])} />
          <line x1={wallA[0][0]} y1={wallA[0][1] - wallH} x2={wallA[1][0]} y2={wallA[1][1] - wallH} />
          <polyline points={pts([[wallB[0][0], wallB[0][1] - wallH], wallB[0], wallB[1], [wallB[1][0], wallB[1][1] - wallH]])} />
          <line x1={wallB[0][0]} y1={wallB[0][1] - wallH} x2={wallB[1][0]} y2={wallB[1][1] - wallH} />
        </g>

        {/* desks, back-to-front so overlaps read correctly */}
        {Object.entries(DESKS)
          .sort((p, q) => p[1].x + p[1].y - (q[1].x + q[1].y))
          .map(([key, d]) => {
            const [lx, ly] = iso(d.x + 0.55, d.y + 0.31)
            return (
              <g key={key}>
                <Desk x={d.x} y={d.y} />
                <text x={lx} y={ly - 56} textAnchor="middle" fontSize="9" letterSpacing="1" fill="var(--faint)">
                  {d.label}
                </text>
              </g>
            )
          })}

        {/* the boss is always present at their desk */}
        {(() => {
          const d = DESKS.sep
          const [px, py] = iso(d.x + 0.5, d.y + 1.05)
          return (
            <g transform={`translate(${px},${py})`}>
              <Figure color="var(--sep)" />
            </g>
          )
        })()}

        {/* agents */}
        {agents.map((a) => {
          const p = posOf(a)
          const [px, py] = iso(p.x, p.y)
          const color = STATUS_COLOR[a.status] ?? STATUS_COLOR.idle
          const showBubble = bubble && bubble.actor === a.key
          return (
            <g key={a.key} className="walker" transform={`translate(${px},${py})`}>
              <Figure color={color} />
              {a.status === 'done' && (
                <text x="0" y="-38" textAnchor="middle" fontSize="10" fill="var(--done)">✓ xong</text>
              )}
              {showBubble && (
                <g>
                  <rect x="8" y="-58" width="150" height="26" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.8" />
                  <polygon points="8,-38 8,-32 16,-38" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.8" />
                  <text x="14" y="-47" fontSize="8.2" fill="var(--ink)">
                    {bubble.text.length > 38 ? bubble.text.slice(0, 37) + '…' : bubble.text}
                  </text>
                </g>
              )}
            </g>
          )
        })}
      </g>
    </svg>
  )
}
