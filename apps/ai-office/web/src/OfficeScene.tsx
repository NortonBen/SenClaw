import { useEffect, useRef, useState } from 'react'
import { traitsFor } from './avatar'
import type { Agent, OfficeEvent } from './types'

/* Isometric wireframe office, mirroring the reel: desks on a 2:1 iso grid,
   one figure per agent, status dots, walk-to-handoff animation and speech
   bubbles. Pure SVG, theme-aware (every color is a CSS variable). */

const ISO_X = 46
const ISO_Y = 23

/* Fixed 9×5 floor. The view orbits freely (any angle): `iso()` rotates a
   layout point around the floor centre before projecting, so the whole room
   — desks, figures, props, walls — turns together. `isoRaw()` is the pure
   projection (rotation-invariant centre only). */
const FW = 9
const FD = 5
const CXL = FW / 2 // floor centre in layout units
const CYL = FD / 2

let ROT_RAD = 0
function setRotation(deg: number) {
  ROT_RAD = (deg * Math.PI) / 180
}

function isoRaw(x: number, y: number, z = 0): [number, number] {
  return [(x - y) * ISO_X, (x + y) * ISO_Y - z]
}

function iso(x: number, y: number, z = 0): [number, number] {
  const c = Math.cos(ROT_RAD)
  const s = Math.sin(ROT_RAD)
  const dx = x - CXL
  const dy = y - CYL
  const rx = CXL + dx * c - dy * s
  const ry = CYL + dx * s + dy * c
  return isoRaw(rx, ry, z)
}

/** Linear interpolate between two screen points (iso is affine along an
 *  edge, so this equals interpolating in world space). */
function lerp(a: [number, number], b: [number, number], f: number): [number, number] {
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
}

function pts(list: [number, number][]): string {
  return list.map(([a, b]) => `${a.toFixed(1)},${b.toFixed(1)}`).join(' ')
}

/** Desk slots spread evenly across the floor: four along the back wall,
 *  manager front-left, QA front-centre, the boss's larger desk front-right. */
const MANAGER_SLOT = { x: 0.9, y: 3.4 }
const QA_SLOT = { x: 4.9, y: 3.4 }
const WORKER_SLOTS = [
  { x: 0.9, y: 1.0 },
  { x: 2.9, y: 1.0 },
  { x: 4.9, y: 1.0 },
  { x: 6.9, y: 1.0 },
  { x: 2.9, y: 3.4 },
]
const SEP_DESK = { x: 6.9, y: 3.5, label: 'SẾP (BẠN)' }

function deskMap(agents: Agent[]): Record<string, { x: number; y: number; label: string }> {
  const map: Record<string, { x: number; y: number; label: string }> = { sep: SEP_DESK }
  let w = 0
  let mgrUsed = false
  let qaUsed = false
  for (const a of agents) {
    if (a.kind === 'manager' && !mgrUsed) {
      map[a.key] = { ...MANAGER_SLOT, label: a.name }
      mgrUsed = true
    } else if (a.kind === 'qa' && !qaUsed) {
      map[a.key] = { ...QA_SLOT, label: a.name }
      qaUsed = true
    } else {
      map[a.key] = { ...(WORKER_SLOTS[w++] ?? MANAGER_SLOT), label: a.name }
    }
  }
  return map
}

const STATUS_COLOR: Record<string, string> = {
  working: 'var(--working)',
  done: 'var(--done)',
  handoff: 'var(--handoff)',
  idle: 'var(--idle)',
}

/** Office chair: seat, backrest and a center leg — tucked at the desk. */
function Chair({ x, y }: { x: number; y: number }) {
  const p = iso(x, y)
  const seatH = 13
  const seat: [number, number][] = [
    [p[0] - 6, p[1] - seatH],
    [p[0] + 6, p[1] - seatH - 3],
    [p[0] + 9, p[1] - seatH - 1],
    [p[0] - 3, p[1] - seatH + 2],
  ]
  return (
    <g stroke="var(--ink)" strokeWidth="0.8" fill="var(--panel)">
      <polygon points={pts(seat)} />
      <polygon
        points={pts([
          [p[0] + 6, p[1] - seatH - 3],
          [p[0] + 9, p[1] - seatH - 1],
          [p[0] + 9, p[1] - seatH - 12],
          [p[0] + 6, p[1] - seatH - 14],
        ])}
      />
      <line x1={p[0] + 1.5} y1={p[1] - seatH + 0.5} x2={p[0] + 1.5} y2={p[1] - 2} />
      <ellipse cx={p[0] + 1.5} cy={p[1] - 2} rx="4.5" ry="1.8" fill="none" />
    </g>
  )
}

/** Wireframe desk: legs, shaded top, monitor (screen lights up while its
 *  owner is working), keyboard, mug and papers. `wide` = the boss's desk. */
function Desk({
  x,
  y,
  active,
  wide = false,
}: {
  x: number
  y: number
  active?: boolean
  wide?: boolean
}) {
  const w = wide ? 1.5 : 1.1
  const d = 0.62
  const h = 26
  const base: [number, number][] = [iso(x, y), iso(x + w, y), iso(x + w, y + d), iso(x, y + d)]
  const top: [number, number][] = base.map(([a, b]) => [a, b - h])

  const monitor = (cx: number) => {
    const c = iso(cx, y + d * 0.38)
    const screen: [number, number][] = [
      [c[0] - 9, c[1] - h - 3],
      [c[0] + 9, c[1] - h - 8],
      [c[0] + 9, c[1] - h - 21],
      [c[0] - 9, c[1] - h - 16],
    ]
    return (
      <g key={cx}>
        <line x1={c[0]} y1={c[1] - h} x2={c[0]} y2={c[1] - h - 4} stroke="var(--ink)" strokeWidth="1" />
        <line x1={c[0] - 4} y1={c[1] - h} x2={c[0] + 4} y2={c[1] - h - 2} stroke="var(--ink)" strokeWidth="1" />
        <polygon
          points={pts(screen)}
          fill={active ? 'var(--working)' : 'var(--paper)'}
          fillOpacity={active ? 0.3 : 1}
          stroke="var(--ink)"
          strokeWidth="1"
        />
        {active && (
          <polygon points={pts(screen)} fill="none" stroke="var(--working)" strokeWidth="0.8" className="pulse" />
        )}
      </g>
    )
  }

  const kb = iso(x + w * 0.5, y + d * 0.72)
  const kbPts: [number, number][] = [
    [kb[0] - 7, kb[1] - h + 1],
    [kb[0] + 7, kb[1] - h - 2.5],
    [kb[0] + 10, kb[1] - h - 1],
    [kb[0] - 4, kb[1] - h + 2.5],
  ]
  const mug = iso(x + w * 0.88, y + d * 0.5)
  const paper = iso(x + w * 0.14, y + d * 0.45)

  return (
    <g strokeWidth="1" stroke="var(--ink)">
      {/* soft shadow under the desk */}
      <polygon
        points={pts([iso(x - 0.06, y + 0.06), iso(x + w + 0.1, y + 0.06), iso(x + w + 0.1, y + d + 0.14), iso(x - 0.06, y + d + 0.14)])}
        fill="var(--ink)"
        fillOpacity="0.06"
        stroke="none"
      />
      {base.map(([a, b], i) => (
        <line key={i} x1={a} y1={b} x2={a} y2={b - h} />
      ))}
      <polygon points={pts(top)} fill="var(--panel)" fillOpacity="0.92" />
      <polygon
        points={pts([top[3], top[2], base[2], base[3]])}
        fill="var(--ink)"
        fillOpacity="0.05"
        stroke="none"
      />
      {wide ? [monitor(x + w * 0.34), monitor(x + w * 0.66)] : monitor(x + w * 0.5)}
      <polygon points={pts(kbPts)} fill="var(--paper)" strokeWidth="0.7" />
      {/* mug */}
      <g strokeWidth="0.7">
        <ellipse cx={mug[0]} cy={mug[1] - h - 4.5} rx="2.2" ry="1" fill="var(--paper)" />
        <line x1={mug[0] - 2.2} y1={mug[1] - h - 4.5} x2={mug[0] - 2.2} y2={mug[1] - h - 1} />
        <line x1={mug[0] + 2.2} y1={mug[1] - h - 4.5} x2={mug[0] + 2.2} y2={mug[1] - h - 1} />
        <ellipse cx={mug[0]} cy={mug[1] - h - 1} rx="2.2" ry="1" fill="none" />
        <path d={`M ${mug[0] + 2.2} ${mug[1] - h - 3.8} q 2.4 0.4 0 2.2`} fill="none" />
      </g>
      {/* papers */}
      <polygon
        points={pts([
          [paper[0] - 4.5, paper[1] - h + 0.5],
          [paper[0] + 3.5, paper[1] - h - 1.8],
          [paper[0] + 6, paper[1] - h - 0.5],
          [paper[0] - 2, paper[1] - h + 1.8],
        ])}
        fill="var(--paper)"
        strokeWidth="0.6"
      />
    </g>
  )
}

/** Scene hairstyles, drawn on the head (center 0,-27, r 4.8). */
function SceneHair({ hair }: { hair: 0 | 1 | 2 | 3 }) {
  switch (hair) {
    case 0: // side part
      return <path d="M -4.7 -27.6 A 4.8 4.8 0 0 1 4.7 -27.6 L 3.2 -29.6 L -4 -30 Z" fill="var(--ink)" opacity="0.75" />
    case 1: // spiky
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M -4.7 -28.4 A 4.8 4.8 0 0 1 4.7 -28.4 Z" />
          <polygon points="-3.4,-30.4 -2.6,-33.2 -1.6,-30.8" />
          <polygon points="-0.8,-31 0,-33.8 0.9,-31" />
          <polygon points="1.7,-30.8 2.7,-33 3.3,-30.3" />
        </g>
      )
    case 2: // bun
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M -4.7 -27.8 A 4.8 4.8 0 0 1 4.7 -27.8 Z" />
          <circle cx="0" cy="-33.4" r="1.8" />
        </g>
      )
    default: // cap with visor
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M -4.7 -28 A 4.8 4.8 0 0 1 4.7 -28 Z" />
          <rect x="0" y="-29" width="7.2" height="1.5" rx="0.75" />
        </g>
      )
  }
}

/** A little person: shadow, accent-tinted shirt, hairstyle + glasses from
 *  the agent's avatar traits, status dot on the chest. Manager/boss get a
 *  tie, QA a clipboard. */
function Figure({
  color,
  working,
  variant,
  agentKey,
}: {
  color: string
  working?: boolean
  variant?: 'manager' | 'qa' | 'boss'
  agentKey: string
}) {
  const t = traitsFor(agentKey)
  return (
    <g className={working ? 'bob' : undefined}>
      <ellipse cx="0" cy="-2.5" rx="8" ry="3.4" fill="var(--ink)" opacity="0.12" />
      <polygon
        points="-5.5,-5 5.5,-5 3.6,-22 -3.6,-22"
        fill="var(--panel)"
        stroke="var(--ink)"
        strokeWidth="1"
      />
      <polygon points="-5.5,-5 5.5,-5 3.6,-22 -3.6,-22" fill={t.color} fillOpacity="0.28" stroke="none" />
      {variant === 'manager' || variant === 'boss' ? (
        <polygon points="-1.2,-22 1.2,-22 0.6,-13 0,-11 -0.6,-13" fill="var(--ink)" fillOpacity="0.55" stroke="none" />
      ) : null}
      <line x1="-5" y1="-17" x2="-7" y2="-10" stroke="var(--ink)" strokeWidth="0.9" />
      <line x1="5" y1="-17" x2="7" y2="-10" stroke="var(--ink)" strokeWidth="0.9" />
      {variant === 'qa' && (
        <rect x="-10.5" y="-16" width="5" height="7" rx="0.5" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.7" transform="rotate(-8)" />
      )}
      <circle cx="0" cy="-27" r="4.8" fill="var(--panel)" stroke="var(--ink)" strokeWidth="1" />
      <SceneHair hair={t.hair} />
      {t.glasses && (
        <g stroke="var(--ink)" strokeWidth="0.6" fill="none">
          <circle cx="-1.9" cy="-26.4" r="1.5" />
          <circle cx="1.9" cy="-26.4" r="1.5" />
          <line x1="-0.4" y1="-26.4" x2="0.4" y2="-26.4" />
        </g>
      )}
      <circle cx="6.5" cy="-20" r="3.4" fill={color} stroke="var(--ink)" strokeWidth="0.6" className={working ? 'pulse' : undefined} />
    </g>
  )
}

/** A back wall standing on floor edge a→b (layout coords), with an optional
 *  window (skyline or sun) and clock. Interpolates along the projected base
 *  so it stays correct at any rotation angle. */
function Wall({
  a,
  b,
  window: win = false,
  sun = false,
  clock = false,
}: {
  a: [number, number]
  b: [number, number]
  window?: boolean
  sun?: boolean
  clock?: boolean
}) {
  const wallH = 78
  const p1 = iso(a[0], a[1])
  const p2 = iso(b[0], b[1])
  const t1: [number, number] = [p1[0], p1[1] - wallH]
  const t2: [number, number] = [p2[0], p2[1] - wallH]
  const zTop = 62
  const zBot = 26
  const zMid = (zTop + zBot) / 2
  const at = (f: number, z: number): [number, number] => {
    const base = lerp(p1, p2, f)
    return [base[0], base[1] - z]
  }
  return (
    <g>
      <polygon points={pts([t1, t2, p2, p1])} fill="var(--ink)" fillOpacity="0.04" stroke="none" />
      <g stroke="var(--line-strong)" strokeWidth="1" fill="none">
        <polyline points={pts([t1, p1, p2, t2])} />
        <line x1={t1[0]} y1={t1[1]} x2={t2[0]} y2={t2[1]} />
      </g>
      {win && (
        <g stroke="var(--line-strong)" strokeWidth="1">
          <polygon
            points={pts([at(0.3, zTop), at(0.7, zTop), at(0.7, zBot), at(0.3, zBot)])}
            fill="var(--handoff)"
            fillOpacity="0.07"
          />
          <line x1={at(0.5, zTop)[0]} y1={at(0.5, zTop)[1]} x2={at(0.5, zBot)[0]} y2={at(0.5, zBot)[1]} />
          <line x1={at(0.3, zMid)[0]} y1={at(0.3, zMid)[1]} x2={at(0.7, zMid)[0]} y2={at(0.7, zMid)[1]} />
          <g stroke="var(--faint)" strokeWidth="0.8" opacity="0.7" fill="none">
            {sun ? (
              <circle cx={at(0.6, 48)[0]} cy={at(0.6, 48)[1]} r="4.5" fill="var(--working)" fillOpacity="0.25" />
            ) : (
              <polyline
                points={pts([
                  at(0.34, 30), at(0.34, 42), at(0.43, 43), at(0.43, 50), at(0.52, 51),
                  at(0.52, 37), at(0.6, 38), at(0.6, 47), at(0.66, 47), at(0.66, 31),
                ])}
              />
            )}
          </g>
        </g>
      )}
      {clock && (
        <g stroke="var(--ink)" strokeWidth="0.9" fill="var(--paper)">
          <ellipse cx={at(0.85, 50)[0]} cy={at(0.85, 50)[1]} rx="6" ry="7" />
          <line x1={at(0.85, 50)[0]} y1={at(0.85, 50)[1]} x2={at(0.85, 54)[0]} y2={at(0.85, 54)[1]} />
          <line x1={at(0.85, 50)[0]} y1={at(0.85, 50)[1]} x2={at(0.87, 49)[0]} y2={at(0.87, 49)[1]} />
        </g>
      )}
    </g>
  )
}

function Plant({ x, y }: { x: number; y: number }) {
  const p = iso(x, y)
  const bx = p[0]
  const by = p[1]
  return (
    <g stroke="var(--ink)" strokeWidth="0.9" fill="var(--panel)">
      <polygon points={pts([[bx - 5, by - 10], [bx + 5, by - 10], [bx + 3.5, by], [bx - 3.5, by]])} />
      <path d={`M ${bx} ${by - 10} C ${bx - 2} ${by - 20}, ${bx - 9} ${by - 22}, ${bx - 10} ${by - 27}`} fill="none" />
      <path d={`M ${bx} ${by - 10} C ${bx + 1} ${by - 22}, ${bx + 7} ${by - 24}, ${bx + 9} ${by - 30}`} fill="none" />
      <path d={`M ${bx} ${by - 10} C ${bx} ${by - 18}, ${bx - 3} ${by - 26}, ${bx + 1} ${by - 32}`} fill="none" />
      <circle cx={bx - 10} cy={by - 28} r="2.6" fill="var(--done)" fillOpacity="0.25" />
      <circle cx={bx + 9} cy={by - 31} r="2.6" fill="var(--done)" fillOpacity="0.25" />
      <circle cx={bx + 1} cy={by - 33} r="2.6" fill="var(--done)" fillOpacity="0.25" />
    </g>
  )
}

/** Pendant lamp hanging from (unseen) ceiling with a light cone hint. */
function Lamp({ x, y }: { x: number; y: number }) {
  const p = iso(x, y)
  const zCord = 118
  const zShade = 92
  return (
    <g stroke="var(--ink)" strokeWidth="0.9">
      <line x1={p[0]} y1={p[1] - zCord} x2={p[0]} y2={p[1] - zShade} />
      <polygon
        points={pts([
          [p[0] - 7, p[1] - zShade + 6],
          [p[0] + 7, p[1] - zShade + 6],
          [p[0] + 3, p[1] - zShade],
          [p[0] - 3, p[1] - zShade],
        ])}
        fill="var(--panel)"
      />
      <ellipse cx={p[0]} cy={p[1] - zShade + 6} rx="7" ry="2.4" fill="var(--working)" fillOpacity="0.22" stroke="none" />
      <ellipse cx={p[0]} cy={p[1]} rx="22" ry="9" fill="var(--working)" fillOpacity="0.05" stroke="none" />
    </g>
  )
}

/** Rug under the boss's corner. */
function Rug() {
  return (
    <g>
      <polygon
        points={pts([iso(6.45, 3.15), iso(8.75, 3.15), iso(8.75, 4.95), iso(6.45, 4.95)])}
        fill="var(--handoff)"
        fillOpacity="0.07"
        stroke="var(--line-strong)"
        strokeWidth="0.8"
        strokeDasharray="3 2"
      />
      <polygon
        points={pts([iso(6.7, 3.35), iso(8.5, 3.35), iso(8.5, 4.75), iso(6.7, 4.75)])}
        fill="none"
        stroke="var(--line)"
        strokeWidth="0.6"
      />
    </g>
  )
}

interface Bubble {
  actor: string
  text: string
  until: number
}

export function OfficeScene({
  agents,
  events,
  rotation = 0,
}: {
  agents: Agent[]
  events: OfficeEvent[]
  rotation?: number
}) {
  setRotation(rotation)
  const [bubble, setBubble] = useState<Bubble | null>(null)

  // While the room is turning, suppress the walk-transition so figures spin
  // together with the desks instead of sliding to catch up.
  const [spin, setSpin] = useState(false)
  const spinTimer = useRef<number | undefined>(undefined)
  const prevRot = useRef(rotation)
  useEffect(() => {
    if (prevRot.current !== rotation) {
      prevRot.current = rotation
      setSpin(true)
      window.clearTimeout(spinTimer.current)
      spinTimer.current = window.setTimeout(() => setSpin(false), 90)
    }
    return () => window.clearTimeout(spinTimer.current)
  }, [rotation])

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

  const desks = deskMap(agents)
  const fallbackDesk = { ...MANAGER_SLOT, label: '' }

  const posOf = (a: Agent): { x: number; y: number } => {
    const home = desks[a.key] ?? fallbackDesk
    if (a.status === 'handoff') {
      const last = [...events]
        .reverse()
        .find((e) => (e.kind === 'bubble' || e.kind === 'handoff') && e.actor === a.key)
      const target = last && desks[last.target] ? desks[last.target] : fallbackDesk
      return { x: target.x - 0.55, y: target.y + 0.4 }
    }
    return { x: home.x + 0.5, y: home.y + 1.05 }
  }

  const tiles: [number, number][] = []
  for (let i = 0; i < FW; i++)
    for (let j = 0; j < FD; j++) if ((i + j) % 2 === 0) tiles.push([i, j])
  const grid: [number, number][][] = []
  for (let i = 0; i <= FW; i++) grid.push([iso(i, 0), iso(i, FD)])
  for (let j = 0; j <= FD; j++) grid.push([iso(0, j), iso(FW, j)])

  // The floor's four edges; the two with the smallest screen-y midpoint are
  // furthest back → they become the walls (so walls never occlude desks).
  const edges: [[number, number], [number, number]][] = [
    [[0, 0], [FW, 0]],
    [[FW, 0], [FW, FD]],
    [[FW, FD], [0, FD]],
    [[0, FD], [0, 0]],
  ]
  const midY = edges.map(([a, b]) => iso((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)[1])
  const backEdges = [0, 1, 2, 3].sort((i, j) => midY[i] - midY[j]).slice(0, 2)

  // Floor centre is rotation-invariant → stable centering offset.
  const floorCenter = isoRaw(CXL, CYL)

  return (
    <svg
      className={`office${spin ? ' spin' : ''}`}
      viewBox="-372 -262 744 486"
      role="img"
      aria-label="Mô phỏng văn phòng"
    >
      <defs>
        <radialGradient id="floorGlow" cx="50%" cy="50%" r="60%">
          <stop offset="0%" stopColor="var(--paper)" stopOpacity="0.55" />
          <stop offset="100%" stopColor="var(--paper)" stopOpacity="0" />
        </radialGradient>
      </defs>
      <g transform={`translate(${(-floorCenter[0]).toFixed(1)},${(-floorCenter[1]).toFixed(1)})`}>
        {/* floor slab + checkerboard + centre glow */}
        <polygon
          points={pts([iso(0, 0), iso(FW, 0), iso(FW, FD), iso(0, FD)])}
          fill="var(--panel)"
          fillOpacity="0.5"
          stroke="none"
        />
        {tiles.map(([i, j]) => (
          <polygon
            key={`${i}-${j}`}
            points={pts([iso(i, j), iso(i + 1, j), iso(i + 1, j + 1), iso(i, j + 1)])}
            fill="var(--ink)"
            fillOpacity="0.028"
            stroke="none"
          />
        ))}
        <ellipse cx={floorCenter[0]} cy={floorCenter[1]} rx="270" ry="105" fill="url(#floorGlow)" stroke="none" />
        <g stroke="var(--line)" strokeWidth="0.7">
          {grid.map(([a, b], i) => (
            <line key={i} x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />
          ))}
        </g>

        {/* two dynamic back walls (chosen per rotation) with window + clock */}
        {backEdges.map((ei, idx) => {
          const [a, b] = edges[ei]
          return <Wall key={ei} a={a} b={b} window sun={idx === 1} clock={idx === 0} />
        })}

        {/* floor decor (rotates with the room) */}
        <Rug />
        <Plant x={8.55} y={0.45} />
        <Plant x={0.45} y={4.6} />
        <Lamp x={2.9} y={2.2} />
        <Lamp x={4.9} y={2.2} />
        <Lamp x={6.9} y={2.2} />

        {/* handoff paths: dashed marching line from walker to target desk */}
        {agents.map((a) => {
          if (!a.enabled || a.status !== 'handoff') return null
          const from = desks[a.key] ?? fallbackDesk
          const p = posOf(a)
          const f = iso(from.x + 0.5, from.y + 1.05)
          const t = iso(p.x, p.y)
          return (
            <line
              key={`path-${a.key}`}
              x1={f[0]}
              y1={f[1] - 2}
              x2={t[0]}
              y2={t[1] - 2}
              stroke="var(--handoff)"
              strokeWidth="1.2"
              strokeDasharray="5 4"
              opacity="0.55"
              className="ants"
            />
          )
        })}

        {/* desks, back-to-front so overlaps read correctly */}
        {Object.entries(desks)
          .sort((p, q) => iso(p[1].x, p[1].y)[1] - iso(q[1].x, q[1].y)[1])
          .map(([key, d]) => {
            const isSep = key === 'sep'
            const agent = agents.find((a) => a.key === key)
            const active = !!agent && agent.enabled && agent.status === 'working'
            const w = isSep ? 1.5 : 1.1
            const [lx, ly] = iso(d.x + w / 2, d.y + 0.31)
            const label = d.label.toUpperCase()
            const plateW = label.length * 6.2 + 10
            return (
              <g key={key} opacity={agent && !agent.enabled ? 0.35 : 1}>
                <Chair x={d.x + w * 0.42} y={d.y + 0.95} />
                <Desk x={d.x} y={d.y} active={active} wide={isSep} />
                {/* name plate */}
                <rect
                  x={lx - plateW / 2}
                  y={ly - 66}
                  width={plateW}
                  height="12"
                  rx="2"
                  fill="var(--paper)"
                  fillOpacity="0.85"
                  stroke="var(--line-strong)"
                  strokeWidth="0.6"
                />
                <text x={lx} y={ly - 57} textAnchor="middle" fontSize="8.4" letterSpacing="1.1" fill="var(--faint)">
                  {label}
                </text>
              </g>
            )
          })}

        {/* the boss is always present at their desk */}
        {(() => {
          const [px, py] = iso(SEP_DESK.x + 0.75, SEP_DESK.y + 1.05)
          return (
            <g transform={`translate(${px},${py})`}>
              <Figure color="var(--sep)" variant="boss" agentKey="sep" />
            </g>
          )
        })()}

        {/* agents */}
        {agents.map((a) => {
          const p = posOf(a)
          const [px, py] = iso(p.x, p.y)
          const color = a.enabled ? (STATUS_COLOR[a.status] ?? STATUS_COLOR.idle) : STATUS_COLOR.idle
          const working = a.enabled && a.status === 'working'
          const showBubble = a.enabled && bubble && bubble.actor === a.key
          const text = showBubble ? (bubble.text.length > 38 ? bubble.text.slice(0, 37) + '…' : bubble.text) : ''
          const bw = 14 + text.length * 4.6
          const variant = a.kind === 'manager' ? 'manager' : a.kind === 'qa' ? 'qa' : undefined
          return (
            <g
              key={a.key}
              className="walker"
              transform={`translate(${px},${py})`}
              opacity={a.enabled ? 1 : 0.3}
            >
              <Figure color={color} working={working} variant={variant} agentKey={a.key} />
              {working && (
                <g fill="var(--working)">
                  <circle cx="-5" cy="-37" r="1.4" className="dot1" />
                  <circle cx="0" cy="-38" r="1.4" className="dot2" />
                  <circle cx="5" cy="-37" r="1.4" className="dot3" />
                </g>
              )}
              {a.enabled && a.status === 'done' && (
                <g>
                  <circle cx="0" cy="-40" r="6.5" fill="var(--done)" fillOpacity="0.14" stroke="var(--done)" strokeWidth="0.8" />
                  <text x="0" y="-37" textAnchor="middle" fontSize="8.5" fill="var(--done)">✓</text>
                </g>
              )}
              {showBubble && (
                <g>
                  <rect x="8" y="-56" width={bw} height="22" rx="4" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.9" />
                  <polygon points="9,-38 9,-30 18,-36" fill="var(--paper)" stroke="var(--ink)" strokeWidth="0.9" />
                  <line x1="9.5" y1="-36.4" x2="16.6" y2="-36.4" stroke="var(--paper)" strokeWidth="1.6" />
                  <text x={8 + bw / 2} y="-41.5" textAnchor="middle" fontSize="8.2" fill="var(--ink)">
                    {text}
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
