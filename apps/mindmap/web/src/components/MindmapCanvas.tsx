import { useEffect, useMemo, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Layout as LayoutStyle, Shape, TreeNode } from '../api'
import { layout, boxWidth, branchColor, PALETTE, SHAPES, type PNode, type PEdge } from '../lib'

const NODE_H = 34
const ROOT_H = 40

export interface StylePatch {
  color?: string | null
  fill?: boolean
  shape?: Shape | null
  icon?: string | null
}

interface Props {
  root: TreeNode | null
  style: LayoutStyle
  selectedId: number | null
  editingId: number | null
  generatingId: number | null
  dragEnabled: boolean
  fullLabels: boolean
  showCount: boolean
  onSelect: (id: number | null) => void
  onStartEdit: (id: number) => void
  onCommitEdit: (id: number, text: string) => void
  onCancelEdit: () => void
  onToggleCollapse: (id: number) => void
  onAddChild: (id: number) => void
  onAddSibling: (id: number) => void
  onDelete: (id: number) => void
  onGenerate: (id: number) => void
  onStyle: (id: number, patch: StylePatch) => void
  onNote: (id: number) => void
  onExpandNote: (id: number) => void
  onContextMenu: (id: number, x: number, y: number) => void
  /** Persist new positions after a free-drag (node + its subtree). */
  onMove: (items: { id: number; x: number; y: number }[]) => void
}

interface View {
  x: number
  y: number
  k: number
}

export default function MindmapCanvas(p: Props) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 })
  const [panning, setPanning] = useState(false)
  const [styleOpen, setStyleOpen] = useState(false)
  const [size, setSize] = useState({ w: 0, h: 0 })
  const [drag, setDrag] = useState<{ dx: number; dy: number } | null>(null)
  const affectedRef = useRef<Set<number>>(new Set())

  const { nodes, edges, widths, branchOf, childrenMap } = useMemo(() => {
    const l = layout(p.root, p.style)
    const widths = new Map<number, number>()
    for (const n of l.nodes) widths.set(n.id, boxWidth(n.text || ' ', n.side === 0, n.icon, p.fullLabels))
    const rootBranch = new Map<number, string>()
    if (p.root && !p.root.collapsed) {
      p.root.children.forEach((c, i) => rootBranch.set(c.id, branchColor(i)))
    }
    const branchOf = new Map<number, string>()
    const byId = new Map(l.nodes.map((n) => [n.id, n]))
    const resolve = (id: number): string => {
      if (branchOf.has(id)) return branchOf.get(id)!
      const n = byId.get(id)
      if (!n) return branchColor(0)
      let c: string
      if (n.color) c = n.color
      else if (rootBranch.has(id)) c = rootBranch.get(id)!
      else if (n.parentId != null) c = resolve(n.parentId)
      else c = 'var(--accent)'
      branchOf.set(id, c)
      return c
    }
    for (const n of l.nodes) resolve(n.id)
    // parent → direct children (for subtree drag)
    const childrenMap = new Map<number, number[]>()
    for (const n of l.nodes) {
      if (n.parentId != null) {
        const arr = childrenMap.get(n.parentId) ?? []
        arr.push(n.id)
        childrenMap.set(n.parentId, arr)
      }
    }
    return { nodes: l.nodes, edges: l.edges, widths, branchOf, childrenMap }
  }, [p.root, p.style, p.fullLabels])

  const rootId = p.root?.id ?? null
  const fittedFor = useRef<string | null>(null)
  useEffect(() => {
    const key = `${rootId}:${p.style}`
    if (rootId == null || nodes.length === 0) return
    if (fittedFor.current === key) return
    // Retry across a few frames until the canvas has a real (non-zero) size —
    // the grid row height can be 0 on the first frame after a map opens.
    let tries = 0
    let raf = 0
    const attempt = () => {
      if (fittedFor.current === key) return
      if (fit()) {
        fittedFor.current = key
        return
      }
      if (tries++ < 30) raf = requestAnimationFrame(attempt)
    }
    raf = requestAnimationFrame(attempt)
    return () => cancelAnimationFrame(raf)
    // size.* is included so we re-attempt once the canvas gets a real size
    // (e.g. when the app/iframe becomes visible after being 0×0).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rootId, p.style, nodes.length, size.w, size.h])

  useEffect(() => {
    setStyleOpen(false)
  }, [p.selectedId])

  // Track the canvas size so overlays can be clamped on-screen.
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }))
    ro.observe(el)
    setSize({ w: el.clientWidth, h: el.clientHeight })
    return () => ro.disconnect()
  }, [])

  function fit(): boolean {
    const el = wrapRef.current
    const cw = el?.clientWidth || size.w
    const ch = el?.clientHeight || size.h
    if (nodes.length === 0 || cw < 20 || ch < 20) return false
    let minX = Infinity
    let minY = Infinity
    let maxX = -Infinity
    let maxY = -Infinity
    for (const n of nodes) {
      const w = widths.get(n.id) ?? 80
      minX = Math.min(minX, n.x - w / 2)
      maxX = Math.max(maxX, n.x + w / 2)
      minY = Math.min(minY, n.y - NODE_H)
      maxY = Math.max(maxY, n.y + NODE_H)
    }
    const pad = 60
    const w = Math.max(1, maxX - minX + pad * 2)
    const h = Math.max(1, maxY - minY + pad * 2)
    const k = Math.max(0.1, Math.min(1.1, Math.min(cw / w, ch / h)))
    const cx = (minX + maxX) / 2
    const cy = (minY + maxY) / 2
    setView({ x: cw / 2 - cx * k, y: ch / 2 - cy * k, k })
    return true
  }

  function onPointerDown(e: React.PointerEvent) {
    if ((e.target as HTMLElement).closest('.node-box, .node-toolbar, .style-panel')) return
    if (e.button !== 0) return
    p.onSelect(null)
    setPanning(true)
    const start = { x: e.clientX, y: e.clientY }
    const v0 = { ...view }
    const move = (ev: PointerEvent) => {
      setView({ x: v0.x + (ev.clientX - start.x), y: v0.y + (ev.clientY - start.y), k: v0.k })
    }
    const up = () => {
      setPanning(false)
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  function onWheel(e: React.WheelEvent) {
    const el = wrapRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const mx = e.clientX - rect.left
    const my = e.clientY - rect.top
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1
    setView((v) => {
      const k = Math.max(0.15, Math.min(3, v.k * factor))
      const scale = k / v.k
      return { k, x: mx - (mx - v.x) * scale, y: my - (my - v.y) * scale }
    })
  }

  const zoomBy = (factor: number) => {
    const el = wrapRef.current
    if (!el) return
    const mx = el.clientWidth / 2
    const my = el.clientHeight / 2
    setView((v) => {
      const k = Math.max(0.15, Math.min(3, v.k * factor))
      const scale = k / v.k
      return { k, x: mx - (mx - v.x) * scale, y: my - (my - v.y) * scale }
    })
  }

  // ---- node interaction: select, or free-drag when enabled ----
  const subtreeIds = (id: number): Set<number> => {
    const out = new Set<number>([id])
    const queue = [id]
    while (queue.length) {
      const cur = queue.shift()!
      for (const c of childrenMap.get(cur) ?? []) {
        if (!out.has(c)) {
          out.add(c)
          queue.push(c)
        }
      }
    }
    return out
  }

  function onNodePointerDown(id: number, e: React.PointerEvent) {
    e.stopPropagation()
    p.onSelect(id)
    if (!p.dragEnabled) return
    affectedRef.current = subtreeIds(id)
    const start = { x: e.clientX, y: e.clientY }
    let moved = false
    const move = (ev: PointerEvent) => {
      const dx = (ev.clientX - start.x) / view.k
      const dy = (ev.clientY - start.y) / view.k
      if (Math.abs(ev.clientX - start.x) + Math.abs(ev.clientY - start.y) > 3) moved = true
      setDrag({ dx, dy })
    }
    const up = (ev: PointerEvent) => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      setDrag(null)
      if (!moved) return
      const dx = (ev.clientX - start.x) / view.k
      const dy = (ev.clientY - start.y) / view.k
      const items = [...affectedRef.current].map((nid) => {
        const n = nodes.find((x) => x.id === nid)!
        return { id: nid, x: n.x + dx, y: n.y + dy }
      })
      p.onMove(items)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  // Apply the live drag offset to affected nodes for rendering.
  const isAff = (id: number) => drag != null && affectedRef.current.has(id)
  const dnodes = drag ? nodes.map((n) => (isAff(n.id) ? { ...n, x: n.x + drag.dx, y: n.y + drag.dy } : n)) : nodes
  const dById = new Map(dnodes.map((n) => [n.id, n]))
  const sel = dnodes.find((n) => n.id === p.selectedId) ?? null

  return (
    <div className="canvas-wrap" ref={wrapRef} onWheel={onWheel} onPointerDown={onPointerDown}>
      {!p.root && (
        <div className="empty">
          <div className="big">🧠</div>
          <div>Chọn hoặc tạo một mindmap để bắt đầu.</div>
        </div>
      )}
      <svg className={panning ? 'panning' : ''}>
        <g transform={`translate(${view.x},${view.y}) scale(${view.k})`}>
          {edges.map((e) => (
            <EdgePath
              key={`${e.from}-${e.to}`}
              edge={e}
              from={dById.get(e.from)!}
              to={dById.get(e.to)!}
              wFrom={widths.get(e.from) ?? 80}
              wTo={widths.get(e.to) ?? 80}
              color={branchOf.get(e.to) ?? 'var(--accent)'}
            />
          ))}
          {dnodes.map((n) => (
            <NodeBox
              key={n.id}
              n={n}
              w={widths.get(n.id) ?? 80}
              color={branchOf.get(n.id) ?? 'var(--accent)'}
              selected={n.id === p.selectedId}
              editing={n.id === p.editingId}
              generating={n.id === p.generatingId}
              draggable={p.dragEnabled}
              showCount={p.showCount}
              onPointerDownNode={(e) => onNodePointerDown(n.id, e)}
              onContextMenu={(e) => {
                e.preventDefault()
                e.stopPropagation()
                p.onSelect(n.id)
                p.onContextMenu(n.id, e.clientX, e.clientY)
              }}
              onStartEdit={() => p.onStartEdit(n.id)}
              onCommit={(t) => p.onCommitEdit(n.id, t)}
              onCancel={p.onCancelEdit}
              onToggle={() => p.onToggleCollapse(n.id)}
            />
          ))}
        </g>
      </svg>

      {sel && p.editingId == null && !drag && (
        <>
          <Toolbar
            node={sel}
            view={view}
            bounds={size}
            generating={p.generatingId === sel.id}
            styleOpen={styleOpen}
            onAddChild={() => p.onAddChild(sel.id)}
            onAddSibling={() => p.onAddSibling(sel.id)}
            onEdit={() => p.onStartEdit(sel.id)}
            onGenerate={() => p.onGenerate(sel.id)}
            onDelete={() => p.onDelete(sel.id)}
            onNote={() => p.onNote(sel.id)}
            onToggleStyle={() => setStyleOpen((v) => !v)}
          />
          {styleOpen && (
            <StylePanel
              node={sel}
              color={branchOf.get(sel.id) ?? 'var(--accent)'}
              view={view}
              bounds={size}
              onStyle={(patch) => p.onStyle(sel.id, patch)}
            />
          )}
          {!styleOpen && sel.note.trim() && (
            <NotePreview
              node={sel}
              view={view}
              bounds={size}
              onExpand={() => p.onExpandNote(sel.id)}
              onEdit={() => p.onNote(sel.id)}
            />
          )}
        </>
      )}

      <div className="zoom">
        <button onClick={() => zoomBy(1.2)} title="Phóng to">
          +
        </button>
        <button onClick={() => zoomBy(1 / 1.2)} title="Thu nhỏ">
          −
        </button>
        <button onClick={fit} title="Vừa màn hình" style={{ fontSize: 13 }}>
          ⤢
        </button>
      </div>
    </div>
  )
}

function nodeHeight(n: PNode): number {
  return n.side === 0 ? ROOT_H : NODE_H
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v))
}

function EdgePath({
  edge,
  from,
  to,
  wFrom,
  wTo,
  color,
}: {
  edge: PEdge
  from: PNode
  to: PNode
  wFrom: number
  wTo: number
  color: string
}) {
  const hFrom = nodeHeight(from)
  const hTo = nodeHeight(to)
  let d: string
  if (edge.kind === 'bezier-v') {
    const x1 = from.x
    const y1 = from.y + hFrom / 2
    const x2 = to.x
    const y2 = to.y - hTo / 2
    const dy = Math.abs(y2 - y1) * 0.5
    d = `M ${x1} ${y1} C ${x1} ${y1 + dy}, ${x2} ${y2 - dy}, ${x2} ${y2}`
  } else if (edge.kind === 'elbow') {
    const gx = from.x - wFrom / 2 + 12
    const y1 = from.y + hFrom / 2
    const x2 = to.x - wTo / 2
    const y2 = to.y
    d = `M ${gx} ${y1} L ${gx} ${y2} L ${x2} ${y2}`
  } else {
    const fromRoot = from.side === 0
    const x1 = fromRoot ? from.x + (to.side * wFrom) / 2 : from.x + (from.side * wFrom) / 2
    const y1 = from.y
    const x2 = to.x - (to.side * wTo) / 2
    const y2 = to.y
    const dx = Math.abs(x2 - x1) * 0.5
    const s = to.side || 1
    d = `M ${x1} ${y1} C ${x1 + s * dx} ${y1}, ${x2 - s * dx} ${y2}, ${x2} ${y2}`
  }
  return <path d={d} fill="none" stroke={color} strokeWidth={2} strokeOpacity={0.55} />
}

function shapeStyle(shape: Shape | null): React.CSSProperties {
  switch (shape) {
    case 'rect':
      return { borderRadius: 4 }
    case 'pill':
      return { borderRadius: 999 }
    case 'ellipse':
      return { borderRadius: '50%', paddingLeft: 16, paddingRight: 16 }
    case 'line':
      return {}
    default:
      return { borderRadius: 10 }
  }
}

function NodeBox({
  n,
  w,
  color,
  selected,
  editing,
  generating,
  draggable,
  showCount,
  onPointerDownNode,
  onContextMenu,
  onStartEdit,
  onCommit,
  onCancel,
  onToggle,
}: {
  n: PNode
  w: number
  color: string
  selected: boolean
  editing: boolean
  generating: boolean
  draggable: boolean
  showCount: boolean
  onPointerDownNode: (e: React.PointerEvent) => void
  onContextMenu: (e: React.MouseEvent) => void
  onStartEdit: () => void
  onCommit: (t: string) => void
  onCancel: () => void
  onToggle: () => void
}) {
  const isRoot = n.side === 0
  const h = nodeHeight(n)
  const hasChildren = n.childCount > 0
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (editing) {
      const el = inputRef.current
      if (el) {
        el.focus()
        el.select()
      }
    }
  }, [editing])

  const filled = isRoot || n.fill
  const isLine = n.shape === 'line' && !isRoot
  const style: React.CSSProperties = { ...shapeStyle(isRoot ? 'rounded' : n.shape) }
  if (draggable) style.cursor = 'grab'
  if (isLine) {
    style.background = 'transparent'
    style.border = 'none'
    style.borderBottom = `2.5px solid ${color}`
    style.borderRadius = 0
    style.boxShadow = 'none'
    style.color = 'var(--text)'
  } else if (filled) {
    style.background = color
    style.border = 'none'
    style.color = '#fff'
  } else {
    style.borderColor = color
  }

  const badge = n.collapsed ? `+${n.childCount}` : showCount ? `${n.childCount}` : '–'

  return (
    <foreignObject x={n.x - w / 2} y={n.y - h / 2} width={w} height={h} style={{ overflow: 'visible' }}>
      <div
        className={`node-box${isRoot ? ' root' : ''}${filled ? ' filled' : ''}${selected ? ' selected' : ''}`}
        style={style}
        onPointerDown={onPointerDownNode}
        onContextMenu={onContextMenu}
        onDoubleClick={(e) => {
          e.stopPropagation()
          onStartEdit()
        }}
      >
        {n.icon ? (
          <span style={{ flexShrink: 0 }}>{n.icon}</span>
        ) : (
          !isRoot &&
          !filled &&
          !isLine && (
            <span style={{ width: 8, height: 8, borderRadius: '50%', background: color, flexShrink: 0 }} />
          )
        )}
        {editing ? (
          <input
            ref={inputRef}
            className="node-input"
            defaultValue={n.text}
            onPointerDown={(e) => e.stopPropagation()}
            onDoubleClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                onCommit((e.target as HTMLInputElement).value)
              } else if (e.key === 'Escape') {
                onCancel()
              }
            }}
            onBlur={(e) => onCommit(e.target.value)}
          />
        ) : (
          <span className="label">{n.text || '…'}</span>
        )}
        {n.note && (
          <span className="note-dot" title={n.note}>
            📝
          </span>
        )}
        {generating && <span className="spin" />}
        {!editing && hasChildren && !isRoot && (
          <span
            className="badge"
            title={n.collapsed ? 'Mở rộng' : 'Thu gọn'}
            onPointerDown={(e) => {
              e.stopPropagation()
              onToggle()
            }}
            style={{ cursor: 'pointer' }}
          >
            {badge}
          </span>
        )}
      </div>
    </foreignObject>
  )
}

function Toolbar({
  node,
  view,
  bounds,
  generating,
  styleOpen,
  onAddChild,
  onAddSibling,
  onEdit,
  onGenerate,
  onDelete,
  onNote,
  onToggleStyle,
}: {
  node: PNode
  view: View
  bounds: { w: number; h: number }
  generating: boolean
  styleOpen: boolean
  onAddChild: () => void
  onAddSibling: () => void
  onEdit: () => void
  onGenerate: () => void
  onDelete: () => void
  onNote: () => void
  onToggleStyle: () => void
}) {
  const isRoot = node.side === 0
  const h = nodeHeight(node)
  const TB_HALF = 130
  const sx = view.x + node.x * view.k
  const nodeTop = view.y + (node.y - h / 2) * view.k
  const nodeBottom = view.y + (node.y + h / 2) * view.k
  // Prefer above the node; flip below if too close to the top edge.
  let top = nodeTop - 46
  if (top < 6) top = nodeBottom + 8
  const left = clamp(sx, TB_HALF + 6, bounds.w - TB_HALF - 6)
  return (
    <div className="node-toolbar" style={{ left, top }}>
      <button className="gen" onClick={onGenerate} title="AI mở rộng nhánh này" disabled={generating}>
        {generating ? <span className="spin" /> : '✨'} AI
      </button>
      <div className="sep" />
      <button onClick={onAddChild} title="Thêm nhánh con (Tab)">
        ＋
      </button>
      {!isRoot && (
        <button onClick={onAddSibling} title="Thêm nhánh ngang (Enter)">
          ↵
        </button>
      )}
      <button onClick={onEdit} title="Sửa tên (F2)">
        ✎
      </button>
      <button onClick={onNote} title="Ghi chú">
        📝
      </button>
      <button className={styleOpen ? 'gen' : ''} onClick={onToggleStyle} title="Kiểu & màu sắc">
        🎨
      </button>
      {!isRoot && (
        <>
          <div className="sep" />
          <button className="danger" onClick={onDelete} title="Xoá (Del)">
            🗑
          </button>
        </>
      )}
    </div>
  )
}

// A diverse, curated emoji set for node icons (scrollable picker).
const EMOJIS = [
  // status & priority
  '⭐', '✅', '❌', '⚠️', '❗', '❓', '🔥', '🎯', '🚩', '🏆', '⏰', '📅',
  // ideas & work
  '💡', '📌', '📎', '📝', '📋', '🔖', '📊', '📈', '📉', '💰', '💵', '🧩',
  // people & roles
  '👤', '👥', '🧑‍💻', '👨‍💼', '🙋', '🤝', '🧠', '👀',
  // tech
  '⚙️', '🔧', '🛠️', '💻', '📱', '🌐', '☁️', '🔌', '🗄️', '🔒', '🔑', '🐛',
  // communication
  '💬', '📧', '📞', '📣', '🔔', '🔗',
  // nature & misc
  '🌱', '🌍', '☀️', '🌙', '💧', '⚡', '🎨', '🎵', '📷', '🎬', '🍀', '🚀',
  // feelings & flags
  '❤️', '👍', '👎', '😀', '💪', '🙏', '✨', '🟢', '🟡', '🔴',
]

function StylePanel({
  node,
  color,
  view,
  bounds,
  onStyle,
}: {
  node: PNode
  color: string
  view: View
  bounds: { w: number; h: number }
  onStyle: (patch: StylePatch) => void
}) {
  const isRoot = node.side === 0
  const h = nodeHeight(node)
  const PANEL_W = 244
  const PANEL_H = isRoot ? 210 : 300
  const sx = view.x + node.x * view.k
  const nodeBottom = view.y + (node.y + h / 2) * view.k
  const nodeTop = view.y + (node.y - h / 2) * view.k
  // Prefer below the node; flip above if it would overflow the bottom.
  let top = nodeBottom + 12
  if (top + PANEL_H > bounds.h - 6) top = nodeTop - PANEL_H - 46
  top = clamp(top, 6, Math.max(6, bounds.h - PANEL_H - 6))
  const left = clamp(sx, PANEL_W / 2 + 6, bounds.w - PANEL_W / 2 - 6)
  return (
    <div className="style-panel" style={{ left, top, width: PANEL_W }} onPointerDown={(e) => e.stopPropagation()}>
      <div className="sp-row sp-swatches">
        {PALETTE.map((c) => (
          <button
            key={c}
            className={`swatch${node.color === c ? ' on' : ''}`}
            style={{ background: c }}
            title={c}
            onClick={() => onStyle({ color: c })}
          />
        ))}
      </div>

      {!isRoot && (
        <div className="sp-row">
          <label className="sp-toggle">
            <input type="checkbox" checked={node.fill} onChange={(e) => onStyle({ fill: e.target.checked })} />
            Tô nền
          </label>
          <span className="sp-sep" />
          {SHAPES.map((s) => (
            <button
              key={s.id}
              className={`sp-shape${(node.shape ?? 'rounded') === s.id ? ' on' : ''}`}
              title={s.label}
              onClick={() => onStyle({ shape: s.id })}
              style={{ borderColor: color }}
            >
              <ShapeIcon shape={s.id} />
            </button>
          ))}
        </div>
      )}

      <div className="sp-icons">
        <div className="sp-icons-head">
          <span>Icon</span>
          <button
            className={`sp-default${!node.icon ? ' on' : ''}`}
            title="Không icon (mặc định)"
            onClick={() => onStyle({ icon: null })}
          >
            ⊘ Mặc định
          </button>
        </div>
        <div className="sp-icons-grid">
          {EMOJIS.map((ic) => (
            <button
              key={ic}
              className={`sp-emoji${node.icon === ic ? ' on' : ''}`}
              onClick={() => onStyle({ icon: node.icon === ic ? null : ic })}
            >
              {ic}
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}

function NotePreview({
  node,
  view,
  bounds,
  onExpand,
  onEdit,
}: {
  node: PNode
  view: View
  bounds: { w: number; h: number }
  onExpand: () => void
  onEdit: () => void
}) {
  const h = nodeHeight(node)
  const W = Math.max(220, Math.min(360, bounds.w - 24))
  const sx = view.x + node.x * view.k
  const nodeBottom = view.y + (node.y + h / 2) * view.k
  let top = nodeBottom + 10
  // Keep it on-screen: if it would overflow the bottom, pin it near the bottom.
  top = clamp(top, 6, Math.max(6, bounds.h - 90))
  const left = clamp(sx, W / 2 + 6, bounds.w - W / 2 - 6)
  return (
    <div
      className="note-preview"
      style={{ left, top, width: W }}
      onPointerDown={(e) => e.stopPropagation()}
      onDoubleClick={onExpand}
    >
      <div className="np-head">
        <span className="np-title">📝 Ghi chú</span>
        <div className="np-actions">
          <button title="Sửa" onClick={onEdit}>
            ✎
          </button>
          <button title="Mở rộng" onClick={onExpand}>
            ⤢
          </button>
        </div>
      </div>
      <div className="np-body markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{node.note}</ReactMarkdown>
      </div>
    </div>
  )
}

function ShapeIcon({ shape }: { shape: Shape }) {
  const common = { width: 22, height: 14, viewBox: '0 0 22 14' }
  switch (shape) {
    case 'line':
      return (
        <svg {...common}>
          <line x1="3" y1="7" x2="19" y2="7" stroke="currentColor" strokeWidth="2" />
        </svg>
      )
    case 'rect':
      return (
        <svg {...common}>
          <rect x="3" y="2" width="16" height="10" fill="none" stroke="currentColor" strokeWidth="1.6" />
        </svg>
      )
    case 'pill':
      return (
        <svg {...common}>
          <rect x="2" y="2" width="18" height="10" rx="5" fill="none" stroke="currentColor" strokeWidth="1.6" />
        </svg>
      )
    case 'ellipse':
      return (
        <svg {...common}>
          <ellipse cx="11" cy="7" rx="9" ry="5.5" fill="none" stroke="currentColor" strokeWidth="1.6" />
        </svg>
      )
    default:
      return (
        <svg {...common}>
          <rect x="3" y="2" width="16" height="10" rx="3" fill="none" stroke="currentColor" strokeWidth="1.6" />
        </svg>
      )
  }
}
