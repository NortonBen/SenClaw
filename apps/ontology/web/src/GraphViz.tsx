import { useMemo, useState } from 'react'
import type { GraphViz as GraphData } from './api'

// Dependency-free, deterministic force-directed layout. Seeds node positions on
// a circle (by index, no randomness) then relaxes with a few dozen iterations of
// repulsion + spring so re-renders are stable.
const CAP = 220

function layout(data: GraphData, w: number, h: number) {
  // When there are more nodes than we can draw, keep the most-connected ones
  // (degree-ranked) rather than an arbitrary lexicographic prefix, so the
  // visible subgraph stays meaningful.
  let nodes = data.nodes
  if (nodes.length > CAP) {
    const deg = new Map<string, number>()
    for (const e of data.edges) {
      deg.set(e.source, (deg.get(e.source) || 0) + 1)
      deg.set(e.target, (deg.get(e.target) || 0) + 1)
    }
    nodes = [...data.nodes].sort((a, b) => (deg.get(b.id) || 0) - (deg.get(a.id) || 0)).slice(0, CAP)
  }
  const idset = new Set(nodes.map((n) => n.id))
  const edges = data.edges.filter((e) => idset.has(e.source) && idset.has(e.target))
  const n = nodes.length
  const pos = new Map<string, { x: number; y: number }>()
  const cx = w / 2
  const cy = h / 2
  const R = Math.min(w, h) * 0.42
  nodes.forEach((node, i) => {
    const a = (i / Math.max(1, n)) * Math.PI * 2
    pos.set(node.id, { x: cx + R * Math.cos(a), y: cy + R * Math.sin(a) })
  })
  const K = Math.sqrt((w * h) / Math.max(1, n)) * 0.55
  for (let iter = 0; iter < 90; iter++) {
    const disp = new Map<string, { x: number; y: number }>()
    nodes.forEach((nd) => disp.set(nd.id, { x: 0, y: 0 }))
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const a = pos.get(nodes[i].id)!
        const b = pos.get(nodes[j].id)!
        let dx = a.x - b.x
        let dy = a.y - b.y
        let d = Math.hypot(dx, dy) || 0.01
        const f = (K * K) / d
        dx /= d
        dy /= d
        const da = disp.get(nodes[i].id)!
        const db = disp.get(nodes[j].id)!
        da.x += dx * f
        da.y += dy * f
        db.x -= dx * f
        db.y -= dy * f
      }
    }
    for (const e of edges) {
      const a = pos.get(e.source)!
      const b = pos.get(e.target)!
      let dx = a.x - b.x
      let dy = a.y - b.y
      let d = Math.hypot(dx, dy) || 0.01
      const f = (d * d) / K
      dx /= d
      dy /= d
      const da = disp.get(e.source)!
      const db = disp.get(e.target)!
      da.x -= dx * f
      da.y -= dy * f
      db.x += dx * f
      db.y += dy * f
    }
    const temp = Math.max(2, 40 * (1 - iter / 90))
    nodes.forEach((nd) => {
      const dp = disp.get(nd.id)!
      const d = Math.hypot(dp.x, dp.y) || 0.01
      const p = pos.get(nd.id)!
      p.x += (dp.x / d) * Math.min(d, temp)
      p.y += (dp.y / d) * Math.min(d, temp)
      p.x = Math.max(24, Math.min(w - 24, p.x))
      p.y = Math.max(24, Math.min(h - 24, p.y))
    })
  }
  return { nodes, edges, pos }
}

export function GraphViz({ data }: { data: GraphData }) {
  const W = 1000
  const H = 620
  const { nodes, edges, pos } = useMemo(() => layout(data, W, H), [data])
  const [hover, setHover] = useState<string | null>(null)

  if (!nodes.length) {
    return <div className="empty">No triples to show yet.</div>
  }
  const radius = (kind: string) => (kind === 'class' ? 9 : kind === 'literal' ? 5 : 7)
  return (
    <div className="viz">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="xMidYMid meet">
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill="var(--muted)" opacity="0.5" />
          </marker>
        </defs>
        {edges.map((e, i) => {
          const a = pos.get(e.source)!
          const b = pos.get(e.target)!
          const show = hover === e.source || hover === e.target
          return (
            <g key={i}>
              <line className="edge" x1={a.x} y1={a.y} x2={b.x} y2={b.y} markerEnd="url(#arrow)" strokeWidth={show ? 1.6 : 0.8} />
              {show && <text className="elabel" x={(a.x + b.x) / 2} y={(a.y + b.y) / 2}>{e.label}</text>}
            </g>
          )
        })}
        {nodes.map((nd) => {
          const p = pos.get(nd.id)!
          return (
            <g key={nd.id} onMouseEnter={() => setHover(nd.id)} onMouseLeave={() => setHover(null)} style={{ cursor: 'pointer' }}>
              <circle cx={p.x} cy={p.y} r={radius(nd.kind)} className={`node-${nd.kind}`} />
              {(hover === nd.id || nd.kind === 'class') && (
                <text x={p.x + radius(nd.kind) + 3} y={p.y + 3}>{nd.label}{nd.type ? ` :${nd.type}` : ''}</text>
              )}
            </g>
          )
        })}
      </svg>
    </div>
  )
}
