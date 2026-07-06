// Hand-rolled layout engine — no external graph lib, so the app stays
// self-contained inside the Space-App iframe. Supports four layout styles:
//   mindmap  two-sided radial (root centered, branches left & right)
//   right    horizontal tree, all branches to the right
//   org      top-down org chart
//   outline  indented list / logic chart with elbow connectors
import type { Layout as LayoutStyle, Shape, TreeNode } from './api'

export interface PNode {
  id: number
  text: string
  note: string
  color: string | null
  shape: Shape | null
  fill: boolean
  icon: string | null
  collapsed: boolean
  depth: number
  side: -1 | 0 | 1
  x: number
  y: number
  parentId: number | null
  childCount: number
}

export type EdgeKind = 'bezier-h' | 'bezier-v' | 'elbow'

export interface PEdge {
  from: number
  to: number
  side: -1 | 0 | 1
  color: string | null
  kind: EdgeKind
}

export interface Layout {
  nodes: PNode[]
  edges: PEdge[]
  bbox: { minX: number; minY: number; maxX: number; maxY: number }
  style: LayoutStyle
}

const H_GAP = 210
const V_GAP = 46

function leafCount(n: TreeNode): number {
  if (n.collapsed || n.children.length === 0) return 1
  return n.children.reduce((s, c) => s + leafCount(c), 0)
}

function pnode(n: TreeNode, depth: number, side: -1 | 0 | 1, x: number, y: number): PNode {
  return {
    id: n.id,
    text: n.text,
    note: n.note,
    color: n.color,
    shape: n.shape,
    fill: n.fill,
    icon: n.icon,
    collapsed: n.collapsed,
    depth,
    side,
    x,
    y,
    parentId: null,
    childCount: n.children.length,
  }
}

export function layout(root: TreeNode | null, style: LayoutStyle = 'mindmap'): Layout {
  const empty: Layout = { nodes: [], edges: [], bbox: { minX: 0, minY: 0, maxX: 0, maxY: 0 }, style }
  if (!root) return empty
  let nodes: PNode[]
  let edgeKind: EdgeKind
  switch (style) {
    case 'org':
      nodes = tidy(root, 'y', 168, 92)
      edgeKind = 'bezier-v'
      break
    case 'right':
      nodes = tidy(root, 'x', H_GAP, 44)
      edgeKind = 'bezier-h'
      break
    case 'outline':
      nodes = outline(root)
      edgeKind = 'elbow'
      break
    default:
      nodes = mindmap(root)
      edgeKind = 'bezier-h'
  }
  // Free-drag overrides: any node with a saved position wins over auto-layout.
  const pos = new Map<number, { x: number; y: number }>()
  const collect = (n: TreeNode) => {
    if (n.pos_x != null && n.pos_y != null) pos.set(n.id, { x: n.pos_x, y: n.pos_y })
    if (!n.collapsed) n.children.forEach(collect)
  }
  collect(root)
  if (pos.size) {
    for (const n of nodes) {
      const p = pos.get(n.id)
      if (p) {
        n.x = p.x
        n.y = p.y
      }
    }
  }
  const edges = buildEdges(root, nodes, edgeKind)
  const bbox = { minX: 0, minY: 0, maxX: 0, maxY: 0 }
  for (const n of nodes) {
    bbox.minX = Math.min(bbox.minX, n.x)
    bbox.maxX = Math.max(bbox.maxX, n.x)
    bbox.minY = Math.min(bbox.minY, n.y)
    bbox.maxY = Math.max(bbox.maxY, n.y)
  }
  return { nodes, edges, bbox, style }
}

// ---- two-sided mind map ----
function mindmap(root: TreeNode): PNode[] {
  const nodes: PNode[] = []
  function place(n: TreeNode, depth: number, side: -1 | 1, cursor: { v: number }): number {
    const visible = n.collapsed ? [] : n.children
    let row: number
    if (visible.length === 0) {
      row = cursor.v
      cursor.v += 1
    } else {
      const rows = visible.map((c) => place(c, depth + 1, side, cursor))
      row = (rows[0] + rows[rows.length - 1]) / 2
    }
    nodes.push(pnode(n, depth, side, side * depth * H_GAP, row * V_GAP))
    return row
  }

  const rootVisible = root.collapsed ? [] : root.children
  const right: TreeNode[] = []
  const left: TreeNode[] = []
  let rl = 0
  let ll = 0
  for (const c of rootVisible) {
    const lc = leafCount(c)
    if (rl <= ll) {
      right.push(c)
      rl += lc
    } else {
      left.push(c)
      ll += lc
    }
  }
  const runSide = (kids: TreeNode[], side: -1 | 1) => {
    const start = nodes.length
    const cursor = { v: 0 }
    for (const c of kids) place(c, 1, side, cursor)
    centerRange(nodes, start, nodes.length)
  }
  runSide(right, 1)
  runSide(left, -1)
  nodes.push(pnode(root, 0, 0, 0, 0))
  return nodes
}

// ---- tidy tree (horizontal 'x' for right, vertical 'y' for org) ----
function tidy(root: TreeNode, depthAxis: 'x' | 'y', gapDepth: number, gapCross: number): PNode[] {
  const nodes: PNode[] = []
  const cursor = { v: 0 }
  function place(n: TreeNode, depth: number): number {
    const visible = n.collapsed ? [] : n.children
    let c: number
    if (visible.length === 0) {
      c = cursor.v
      cursor.v += 1
    } else {
      const cs = visible.map((ch) => place(ch, depth + 1))
      c = (cs[0] + cs[cs.length - 1]) / 2
    }
    const dp = depth * gapDepth
    const cp = c * gapCross
    const x = depthAxis === 'x' ? dp : cp
    const y = depthAxis === 'x' ? cp : dp
    nodes.push(pnode(n, depth, depth === 0 ? 0 : 1, x, y))
    return c
  }
  place(root, 0)
  // center the cross axis around 0
  if (depthAxis === 'x') centerRangeY(nodes, 0, nodes.length)
  else centerRangeX(nodes, 0, nodes.length)
  return nodes
}

// ---- indented outline / list ----
function outline(root: TreeNode): PNode[] {
  const INDENT = 30
  const ROW = 36
  const nodes: PNode[] = []
  let row = 0
  function place(n: TreeNode, depth: number) {
    nodes.push(pnode(n, depth, depth === 0 ? 0 : 1, depth * INDENT, row * ROW))
    row += 1
    if (!n.collapsed) n.children.forEach((c) => place(c, depth + 1))
  }
  place(root, 0)
  return nodes
}

function buildEdges(root: TreeNode, nodes: PNode[], kind: EdgeKind): PEdge[] {
  const edges: PEdge[] = []
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const walk = (n: TreeNode, parentColor: string | null) => {
    const color = n.color ?? parentColor
    if (n.collapsed) return
    for (const c of n.children) {
      const cn = byId.get(c.id)
      if (cn) {
        cn.parentId = n.id
        edges.push({ from: n.id, to: c.id, side: cn.side, color: c.color ?? color, kind })
      }
      walk(c, color)
    }
  }
  walk(root, root.color ?? null)
  return edges
}

function centerRange(nodes: PNode[], start: number, end: number) {
  if (end <= start) return
  let min = Infinity
  let max = -Infinity
  for (let i = start; i < end; i++) {
    min = Math.min(min, nodes[i].y)
    max = Math.max(max, nodes[i].y)
  }
  const mid = (min + max) / 2
  for (let i = start; i < end; i++) nodes[i].y -= mid
}
function centerRangeY(nodes: PNode[], start: number, end: number) {
  centerRange(nodes, start, end)
}
function centerRangeX(nodes: PNode[], start: number, end: number) {
  if (end <= start) return
  let min = Infinity
  let max = -Infinity
  for (let i = start; i < end; i++) {
    min = Math.min(min, nodes[i].x)
    max = Math.max(max, nodes[i].x)
  }
  const mid = (min + max) / 2
  for (let i = start; i < end; i++) nodes[i].x -= mid
}

export const BRANCH_COLORS = [
  '#f97316',
  '#3b82f6',
  '#10b981',
  '#a855f7',
  '#ef4444',
  '#14b8a6',
  '#eab308',
  '#ec4899',
]

export function branchColor(i: number): string {
  return BRANCH_COLORS[i % BRANCH_COLORS.length]
}

/** Theme palette shown in the style panel (matches the branch colors + neutrals). */
export const PALETTE = [
  '#6d5cf5',
  '#3b82f6',
  '#0ea5e9',
  '#14b8a6',
  '#10b981',
  '#eab308',
  '#f97316',
  '#ef4444',
  '#ec4899',
  '#a855f7',
  '#64748b',
  '#111827',
]

export const SHAPES: { id: Shape; label: string }[] = [
  { id: 'line', label: 'Đường kẻ' },
  { id: 'rect', label: 'Chữ nhật' },
  { id: 'rounded', label: 'Bo góc' },
  { id: 'pill', label: 'Viên thuốc' },
  { id: 'ellipse', label: 'Bầu dục' },
]

export function boxWidth(text: string, root: boolean, icon?: string | null, full = false): number {
  // `base` budgets for padding + the leading dot and trailing count badge so
  // labels aren't clipped prematurely.
  const base = root ? 50 : 52
  const iconw = icon ? 20 : 0
  const per = root ? 10 : 8.6
  const w = base + iconw + text.length * per
  // `full` label mode raises the cap so long labels show completely.
  const cap = full ? 620 : root ? 300 : 240
  return Math.max(root ? 96 : 78, Math.min(w, cap))
}
