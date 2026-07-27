// Trình dựng flow kéo-thả trực quan (no-code) trên react-flow v12.
// Canvas node-based: Source (xanh dương) → Transform (xanh lá) → Export (cam).
// Sinh ra FlowDef JSON đúng shape apps/lakehouse/src/flow.rs rồi lưu qua REST.
import { useCallback, useMemo, useState } from 'react'
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  Controls,
  MiniMap,
  Handle,
  Position,
  addEdge,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type Connection,
  type NodeProps,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import {
  App,
  Button,
  Card,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Radio,
  Select,
  Space,
  Switch,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import {
  ApiOutlined,
  DeleteOutlined,
  DeploymentUnitOutlined,
  ExportOutlined,
  EyeOutlined,
  SaveOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import { useQuery } from '@tanstack/react-query'
import { ApiError, createFlow, listConnections, listDatasets, updateFlow } from '../api'
import { errMsg } from '../util'
import type { FieldError, FlowImpact } from '../types'

// ---- Kiểu dữ liệu node (giữ trong node.data) ----
type NodeKind = 'source' | 'transform' | 'export'

interface BaseData {
  kind: NodeKind
  stepId: string
  errors?: string[] // message lỗi validate gắn vào node
  [key: string]: unknown
}
interface SourceData extends BaseData {
  kind: 'source'
  connection: string
  table?: string
  query?: string
  mode: string
  cursorColumn?: string
  cursorInitial?: string
  primaryKey: string[]
  mergeKey: string[]
  strategy?: string
  allowFullRewrite?: boolean
  hardDeletes?: string
  checkColumns: string[]
  partitionBy: string[]
  columns: string[]
  targetNs?: string
  targetDataset?: string
}
interface TransformData extends BaseData {
  kind: 'transform'
  transformKind: string // full | incremental_by_time
  timeColumn?: string
  interval?: string
  lookback?: number
  sql: string
  targetNs?: string
  targetDataset?: string
}
interface ExportData extends BaseData {
  kind: 'export'
  dest: 'connection' | 'format'
  connection?: string
  table?: string
  format?: string
  mode: string
  keys: string[]
}
type AnyData = SourceData | TransformData | ExportData
type FlowNode = Node<AnyData>

const SOURCE_MODES = ['full_refresh', 'incremental_append', 'incremental_merge', 'snapshot']
const TRANSFORM_KINDS = ['full', 'incremental_by_time']
const INTERVALS = ['hour', 'day', 'week', 'month']
const EXPORT_MODES = ['full_refresh', 'append', 'upsert']
const MERGE_STRATEGIES = ['delete_insert', 'upsert', 'insert_only']
const SNAPSHOT_STRATEGIES = ['timestamp', 'check']
const HARD_DELETES = ['ignore', 'invalidate', 'new_record']

const KIND_COLOR: Record<NodeKind, string> = {
  source: '#1677ff',
  transform: '#52c41a',
  export: '#fa8c16',
}
const KIND_LABEL: Record<NodeKind, string> = {
  source: 'Nguồn',
  transform: 'Biến đổi',
  export: 'Xuất',
}

// slug hợp lệ làm id step (dùng làm alias SQL): [a-z0-9_]
function slug(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 64)
}

function uniqueStepId(base: string, taken: Set<string>): string {
  const b = slug(base) || 'step'
  if (!taken.has(b)) return b
  let i = 2
  while (taken.has(`${b}_${i}`)) i++
  return `${b}_${i}`
}

let seq = 1
const nid = () => `n${seq++}`

// ---- Node component chung ----
function NodeShell({
  kind,
  title,
  summary,
  selected,
  hasError,
  onDelete,
  showTarget,
  showSource,
}: {
  kind: NodeKind
  title: string
  summary: string
  selected?: boolean
  hasError?: boolean
  onDelete: () => void
  showTarget: boolean
  showSource: boolean
}) {
  const color = KIND_COLOR[kind]
  return (
    <div
      style={{
        minWidth: 180,
        maxWidth: 240,
        borderRadius: 10,
        border: `2px solid ${hasError ? '#ff4d4f' : selected ? color : '#d9d9d9'}`,
        background: '#fff',
        boxShadow: selected ? `0 0 0 2px ${color}33` : '0 1px 4px rgba(0,0,0,0.08)',
        overflow: 'hidden',
      }}
    >
      {showTarget && <Handle type="target" position={Position.Left} style={{ background: color }} />}
      <div
        style={{
          background: color,
          color: '#fff',
          padding: '4px 8px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          fontSize: 12,
        }}
      >
        <span style={{ fontWeight: 600 }}>{KIND_LABEL[kind]}</span>
        <Button
          type="text"
          size="small"
          icon={<DeleteOutlined style={{ color: '#fff' }} />}
          onClick={(e) => {
            e.stopPropagation()
            onDelete()
          }}
          style={{ height: 18, width: 18, minWidth: 18, padding: 0 }}
        />
      </div>
      <div style={{ padding: '6px 8px' }}>
        <div
          style={{
            fontWeight: 600,
            fontSize: 13,
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 11, color: '#888', marginTop: 2, wordBreak: 'break-word' }}>
          {summary}
        </div>
        {hasError && (
          <Tag color="error" style={{ marginTop: 4, fontSize: 10, lineHeight: '16px' }}>
            có lỗi
          </Tag>
        )}
      </div>
      {showSource && <Handle type="source" position={Position.Right} style={{ background: color }} />}
    </div>
  )
}

function sourceSummary(d: SourceData): string {
  const where = d.table ? d.table : d.query ? '(query)' : '(chưa đặt)'
  return `${d.connection || '?'} · ${where} · ${d.mode}`
}
function transformSummary(d: TransformData): string {
  return d.transformKind === 'incremental_by_time'
    ? `incremental · ${d.interval ?? '?'}`
    : 'full'
}
function exportSummary(d: ExportData): string {
  const dest =
    d.dest === 'connection' ? `${d.connection || '?'}.${d.table || '?'}` : `file ${d.format || '?'}`
  return `${dest} · ${d.mode}`
}

// Các node component đọc callback onDelete từ data (stable ref).
function SourceNode({ data, selected }: NodeProps<FlowNode>) {
  const d = data as SourceData
  return (
    <NodeShell
      kind="source"
      title={d.stepId}
      summary={sourceSummary(d)}
      selected={selected}
      hasError={!!d.errors?.length}
      onDelete={() => (d.onDelete as () => void)?.()}
      showTarget={false}
      showSource
    />
  )
}
function TransformNode({ data, selected }: NodeProps<FlowNode>) {
  const d = data as TransformData
  return (
    <NodeShell
      kind="transform"
      title={d.stepId}
      summary={transformSummary(d)}
      selected={selected}
      hasError={!!d.errors?.length}
      onDelete={() => (d.onDelete as () => void)?.()}
      showTarget
      showSource
    />
  )
}
function ExportNode({ data, selected }: NodeProps<FlowNode>) {
  const d = data as ExportData
  return (
    <NodeShell
      kind="export"
      title={d.stepId}
      summary={exportSummary(d)}
      selected={selected}
      hasError={!!d.errors?.length}
      onDelete={() => (d.onDelete as () => void)?.()}
      showTarget
      showSource={false}
    />
  )
}

const NODE_TYPES = { source: SourceNode, transform: TransformNode, export: ExportNode }

// ---- factory data mặc định ----
function newSource(stepId: string, connection = ''): SourceData {
  return {
    kind: 'source',
    stepId,
    connection,
    table: '',
    mode: 'full_refresh',
    primaryKey: [],
    mergeKey: [],
    checkColumns: [],
    partitionBy: [],
    columns: [],
  }
}
function newTransform(stepId: string): TransformData {
  return { kind: 'transform', stepId, transformKind: 'full', sql: 'SELECT * FROM ' }
}
function newExport(stepId: string): ExportData {
  return { kind: 'export', stepId, dest: 'format', format: 'csv', mode: 'full_refresh', keys: [] }
}

// ---- token FROM/JOIN (khớp flow.rs referenced_ids) để dựng cạnh khi nạp flow cũ ----
function referencedIds(sql: string, known: Set<string>): string[] {
  const lower = sql.toLowerCase()
  const toks = lower.match(/[a-z0-9_.]+/g) ?? []
  const out = new Set<string>()
  for (let i = 0; i < toks.length; i++) {
    if ((toks[i] === 'from' || toks[i] === 'join') && i + 1 < toks.length) {
      const cand = toks[i + 1]
      if (!cand.includes('.') && known.has(cand)) out.add(cand)
    }
  }
  return [...out]
}

// ---- build FlowDef từ nodes + edges ----
function buildDef(flowName: string, nodes: FlowNode[], edges: Edge[]): Record<string, unknown> {
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const arr = (xs: string[]) => xs.filter((x) => x && x.trim())

  const sources = nodes
    .filter((n) => (n.data as AnyData).kind === 'source')
    .map((n) => {
      const d = n.data as SourceData
      const s: Record<string, unknown> = { id: d.stepId, connection: d.connection, mode: d.mode }
      if (d.table && d.table.trim()) s.table = d.table
      else if (d.query && d.query.trim()) s.query = d.query
      if (d.mode === 'incremental_append' || d.mode === 'incremental_merge') {
        const cur: Record<string, unknown> = { column: d.cursorColumn ?? '' }
        if (d.cursorInitial !== undefined && d.cursorInitial !== '') cur.initial = d.cursorInitial
        s.cursor = cur
      }
      if (d.mode === 'snapshot' && d.strategy === 'timestamp' && d.cursorColumn) {
        s.cursor = { column: d.cursorColumn }
      }
      if (arr(d.primaryKey).length) s.primary_key = arr(d.primaryKey)
      if (arr(d.mergeKey).length) s.merge_key = arr(d.mergeKey)
      if (d.strategy) s.strategy = d.strategy
      if (d.allowFullRewrite) s.allow_full_rewrite = true
      if (d.mode === 'snapshot' && d.hardDeletes) s.hard_deletes = d.hardDeletes
      if (arr(d.checkColumns).length) s.check_columns = arr(d.checkColumns)
      if (arr(d.columns).length) s.columns = arr(d.columns)
      const target: Record<string, unknown> = {}
      if (d.targetNs) target.namespace = d.targetNs
      if (d.targetDataset) target.dataset = d.targetDataset
      if (arr(d.partitionBy).length) target.partition_by = arr(d.partitionBy)
      if (Object.keys(target).length) s.target = target
      return s
    })

  const transforms = nodes
    .filter((n) => (n.data as AnyData).kind === 'transform')
    .map((n) => {
      const d = n.data as TransformData
      const t: Record<string, unknown> = { id: d.stepId, kind: d.transformKind, sql: d.sql }
      if (d.transformKind === 'incremental_by_time') {
        if (d.timeColumn) t.time_column = d.timeColumn
        if (d.interval) t.interval = d.interval
        if (d.lookback !== undefined && d.lookback !== null) t.lookback = d.lookback
      }
      const target: Record<string, unknown> = {}
      if (d.targetNs) target.namespace = d.targetNs
      if (d.targetDataset) target.dataset = d.targetDataset
      if (Object.keys(target).length) t.target = target
      return t
    })

  const exports = nodes
    .filter((n) => (n.data as AnyData).kind === 'export')
    .map((n) => {
      const d = n.data as ExportData
      // input = stepId của node nối vào (cạnh có target = node export này)
      const inEdge = edges.find((e) => e.target === n.id)
      const inNode = inEdge ? byId.get(inEdge.source) : undefined
      const ex: Record<string, unknown> = {
        id: d.stepId,
        input: inNode ? (inNode.data as AnyData).stepId : '',
        mode: d.mode,
      }
      if (d.dest === 'connection') {
        if (d.connection) ex.connection = d.connection
        if (d.table) ex.table = d.table
      } else if (d.format) {
        ex.format = d.format
      }
      if (d.mode === 'upsert' && arr(d.keys).length) ex.keys = arr(d.keys)
      return ex
    })

  return { version: 1, flow: flowName, sources, transforms, exports }
}

// ---- dựng nodes+edges từ FlowDef (nạp flow cũ để sửa) ----
function defToGraph(def: unknown): { nodes: FlowNode[]; edges: Edge[]; flowName: string } {
  const d = (def ?? {}) as {
    flow?: string
    sources?: Array<Record<string, unknown>>
    transforms?: Array<Record<string, unknown>>
    exports?: Array<Record<string, unknown>>
  }
  const nodes: FlowNode[] = []
  const edges: Edge[] = []
  const idToNode = new Map<string, string>() // stepId → node.id

  const push = (data: AnyData, x: number, y: number) => {
    const id = nid()
    idToNode.set(data.stepId, id)
    nodes.push({ id, type: data.kind, position: { x, y }, data })
    return id
  }

  ;(d.sources ?? []).forEach((s, i) => {
    const cur = s.cursor as { column?: string; initial?: unknown } | undefined
    const tgt = s.target as
      | { namespace?: string; dataset?: string; partition_by?: string[] }
      | undefined
    const data: SourceData = {
      kind: 'source',
      stepId: String(s.id ?? `src${i}`),
      connection: String(s.connection ?? ''),
      table: s.table ? String(s.table) : undefined,
      query: s.query ? String(s.query) : undefined,
      mode: String(s.mode ?? 'full_refresh'),
      cursorColumn: cur?.column,
      cursorInitial:
        cur?.initial !== undefined && cur?.initial !== null ? String(cur.initial) : undefined,
      primaryKey: (s.primary_key as string[]) ?? [],
      mergeKey: (s.merge_key as string[]) ?? [],
      strategy: s.strategy ? String(s.strategy) : undefined,
      allowFullRewrite: !!s.allow_full_rewrite,
      hardDeletes: s.hard_deletes ? String(s.hard_deletes) : undefined,
      checkColumns: (s.check_columns as string[]) ?? [],
      partitionBy: tgt?.partition_by ?? [],
      columns: (s.columns as string[]) ?? [],
      targetNs: tgt?.namespace,
      targetDataset: tgt?.dataset,
    }
    push(data, 0, i * 130)
  })
  ;(d.transforms ?? []).forEach((t, i) => {
    const tgt = t.target as { namespace?: string; dataset?: string } | undefined
    const data: TransformData = {
      kind: 'transform',
      stepId: String(t.id ?? `tf${i}`),
      transformKind: String(t.kind ?? 'full'),
      timeColumn: t.time_column ? String(t.time_column) : undefined,
      interval: t.interval ? String(t.interval) : undefined,
      lookback: typeof t.lookback === 'number' ? (t.lookback as number) : undefined,
      sql: String(t.sql ?? ''),
      targetNs: tgt?.namespace,
      targetDataset: tgt?.dataset,
    }
    push(data, 320, i * 130)
  })
  ;(d.exports ?? []).forEach((e, i) => {
    const hasConn = !!e.connection
    const data: ExportData = {
      kind: 'export',
      stepId: String(e.id ?? `exp${i}`),
      dest: hasConn ? 'connection' : 'format',
      connection: e.connection ? String(e.connection) : undefined,
      table: e.table ? String(e.table) : undefined,
      format: e.format ? String(e.format) : 'csv',
      mode: String(e.mode ?? 'full_refresh'),
      keys: (e.keys as string[]) ?? [],
    }
    push(data, 640, i * 130)
  })

  // cạnh export.input
  ;(d.exports ?? []).forEach((e) => {
    const from = idToNode.get(String(e.input ?? ''))
    const to = idToNode.get(String(e.id ?? ''))
    if (from && to) edges.push({ id: `e_${from}_${to}`, source: from, target: to })
  })
  // cạnh transform ← step tham chiếu trong FROM/JOIN (best-effort)
  const known = new Set(idToNode.keys())
  ;(d.transforms ?? []).forEach((t) => {
    const to = idToNode.get(String(t.id ?? ''))
    if (!to) return
    for (const ref of referencedIds(String(t.sql ?? ''), known)) {
      const from = idToNode.get(ref)
      if (from && from !== to) edges.push({ id: `e_${from}_${to}`, source: from, target: to })
    }
  })

  return { nodes, edges, flowName: String(d.flow ?? 'flow_moi') }
}

// ---- phát hiện chu trình client-side ----
function hasCycle(nodes: FlowNode[], edges: Edge[]): boolean {
  const adj = new Map<string, string[]>()
  nodes.forEach((n) => adj.set(n.id, []))
  edges.forEach((e) => adj.get(e.source)?.push(e.target))
  const state = new Map<string, number>() // 0=chưa,1=đang,2=xong
  const dfs = (u: string): boolean => {
    state.set(u, 1)
    for (const v of adj.get(u) ?? []) {
      const s = state.get(v) ?? 0
      if (s === 1) return true
      if (s === 0 && dfs(v)) return true
    }
    state.set(u, 2)
    return false
  }
  for (const n of nodes) if ((state.get(n.id) ?? 0) === 0 && dfs(n.id)) return true
  return false
}

// ============================================================================
export function FlowBuilder({
  mode,
  flowId,
  initialDef,
  onSaved,
  onExportJson,
}: {
  mode: 'create' | 'edit'
  flowId?: string
  initialDef?: unknown
  onSaved: () => void
  onExportJson?: (def: unknown) => void
}) {
  return (
    <ReactFlowProvider>
      <BuilderInner
        mode={mode}
        flowId={flowId}
        initialDef={initialDef}
        onSaved={onSaved}
        onExportJson={onExportJson}
      />
    </ReactFlowProvider>
  )
}

function BuilderInner({
  mode,
  flowId,
  initialDef,
  onSaved,
  onExportJson,
}: {
  mode: 'create' | 'edit'
  flowId?: string
  initialDef?: unknown
  onSaved: () => void
  onExportJson?: (def: unknown) => void
}) {
  const { message } = App.useApp()
  const initial = useMemo(() => defToGraph(initialDef), [initialDef])

  const [flowName, setFlowName] = useState(initial.flowName)
  const [nodes, setNodes, onNodesChange] = useNodesState<FlowNode>(initial.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>(initial.edges)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [dslOpen, setDslOpen] = useState(false)
  const [savedDag, setSavedDag] = useState<string[] | null>(null)

  const connections = useQuery({ queryKey: ['connections'], queryFn: listConnections })
  const datasets = useQuery({ queryKey: ['datasets'], queryFn: () => listDatasets(undefined, 200) })

  const connOpts = (connections.data?.connections ?? []).map((c) => ({
    value: c.id,
    label: `${c.id} (${c.kind})`,
  }))
  const datasetNames = (datasets.data?.datasets ?? []).map((d) => `${d.namespace}.${d.name}`)

  // stable delete callback — functional setState nên không stale.
  const deleteNode = useCallback(
    (id: string) => {
      setNodes((ns) => ns.filter((n) => n.id !== id))
      setEdges((es) => es.filter((e) => e.source !== id && e.target !== id))
      setSelectedId((cur) => (cur === id ? null : cur))
    },
    [setNodes, setEdges],
  )

  // gắn onDelete vào data mỗi node (bọc để node component gọi được).
  const rfNodes = useMemo(
    () =>
      nodes.map((n) => ({
        ...n,
        data: { ...n.data, onDelete: () => deleteNode(n.id) } as AnyData,
      })),
    [nodes, deleteNode],
  )

  const takenIds = useMemo(
    () => new Set(nodes.map((n) => (n.data as AnyData).stepId)),
    [nodes],
  )

  const addNode = useCallback(
    (kind: NodeKind) => {
      const stepId = uniqueStepId(
        kind === 'source' ? 'nguon' : kind === 'transform' ? 'bien_doi' : 'xuat',
        takenIds,
      )
      const data: AnyData =
        kind === 'source'
          ? newSource(stepId, connOpts[0]?.value)
          : kind === 'transform'
            ? newTransform(stepId)
            : newExport(stepId)
      const col = kind === 'source' ? 0 : kind === 'transform' ? 320 : 640
      const count = nodes.filter((n) => (n.data as AnyData).kind === kind).length
      const id = nid()
      setNodes((ns) => [
        ...ns,
        { id, type: kind, position: { x: col, y: count * 130 + 20 }, data },
      ])
      setSelectedId(id)
    },
    [nodes, takenIds, connOpts, setNodes],
  )

  const autoLayout = useCallback(() => {
    const counters: Record<NodeKind, number> = { source: 0, transform: 0, export: 0 }
    setNodes((ns) =>
      ns.map((n) => {
        const k = (n.data as AnyData).kind
        const col = k === 'source' ? 0 : k === 'transform' ? 320 : 640
        const y = counters[k] * 130 + 20
        counters[k]++
        return { ...n, position: { x: col, y } }
      }),
    )
  }, [setNodes])

  const onConnect = useCallback(
    (c: Connection) => {
      // export chỉ nhận 1 input → thay cạnh cũ.
      const targetNode = nodes.find((n) => n.id === c.target)
      setEdges((es) => {
        let next = es
        if (targetNode && (targetNode.data as AnyData).kind === 'export') {
          next = es.filter((e) => e.target !== c.target)
        }
        return addEdge(c, next)
      })
    },
    [nodes, setEdges],
  )

  // cập nhật data của node đang chọn.
  const updateSelected = useCallback(
    (patch: Partial<AnyData>) => {
      if (!selectedId) return
      setNodes((ns) =>
        ns.map((n) =>
          n.id === selectedId ? { ...n, data: { ...n.data, ...patch } as AnyData } : n,
        ),
      )
    },
    [selectedId, setNodes],
  )

  const selected = nodes.find((n) => n.id === selectedId) ?? null

  // upstream stepId của node đang chọn (gợi ý FROM cho transform).
  const upstreamIds = useMemo(() => {
    if (!selected) return []
    return edges
      .filter((e) => e.target === selected.id)
      .map((e) => nodes.find((n) => n.id === e.source))
      .filter(Boolean)
      .map((n) => (n!.data as AnyData).stepId)
  }, [selected, edges, nodes])

  const applyFieldErrors = useCallback(
    (fes: FieldError[]) => {
      setNodes((ns) =>
        ns.map((n) => {
          const sid = (n.data as AnyData).stepId
          const errs = fes.filter((f) => f.step_id === sid).map((f) => `${f.field}: ${f.message}`)
          return { ...n, data: { ...n.data, errors: errs.length ? errs : undefined } as AnyData }
        }),
      )
    },
    [setNodes],
  )

  const clearErrors = useCallback(() => {
    setNodes((ns) => ns.map((n) => ({ ...n, data: { ...n.data, errors: undefined } as AnyData })))
  }, [setNodes])

  const currentDef = () => buildDef(flowName, nodes, edges)

  const doSave = useCallback(async () => {
    clearErrors()
    if (hasCycle(nodes, edges)) {
      message.error('Phát hiện chu trình trong sơ đồ — hãy bỏ bớt cạnh nối vòng')
      return
    }
    const def = currentDef()
    setSaving(true)
    try {
      let dag: string[] | null = null
      if (mode === 'create') {
        const r = await createFlow(def, false)
        dag = r.dag ?? null
      } else {
        try {
          await updateFlow(flowId!, def, false)
        } catch (e) {
          if (e instanceof ApiError && e.status === 409 && e.details) {
            const ok = await confirmReset(e.details as FlowImpact)
            if (!ok) {
              setSaving(false)
              return
            }
            await updateFlow(flowId!, def, true)
          } else {
            throw e
          }
        }
      }
      setSavedDag(dag)
      message.success(mode === 'create' ? 'Đã tạo flow' : 'Đã cập nhật flow')
      onSaved()
    } catch (e) {
      if (e instanceof ApiError && e.status === 400 && Array.isArray(e.details)) {
        applyFieldErrors(e.details as FieldError[])
        message.error('Flow chưa hợp lệ — node lỗi được tô đỏ, xem chi tiết trong node')
      } else {
        message.error(errMsg(e))
      }
    } finally {
      setSaving(false)
    }
  }, [nodes, edges, flowName, mode, flowId])

  const empty = nodes.length === 0

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: 'calc(100vh - 150px)' }}>
      {/* Toolbar */}
      <Space wrap style={{ marginBottom: 8 }}>
        <Input
          addonBefore="Tên flow"
          value={flowName}
          onChange={(e) => setFlowName(slug(e.target.value))}
          style={{ width: 260 }}
          placeholder="flow_moi"
          disabled={mode === 'edit'}
        />
        <Button icon={<ApiOutlined style={{ color: KIND_COLOR.source }} />} onClick={() => addNode('source')}>
          Nguồn
        </Button>
        <Button
          icon={<ThunderboltOutlined style={{ color: KIND_COLOR.transform }} />}
          onClick={() => addNode('transform')}
        >
          Biến đổi
        </Button>
        <Button icon={<ExportOutlined style={{ color: KIND_COLOR.export }} />} onClick={() => addNode('export')}>
          Xuất
        </Button>
        <Tooltip title="Sắp xếp lại: nguồn trái → biến đổi giữa → xuất phải">
          <Button icon={<DeploymentUnitOutlined />} onClick={autoLayout}>
            Tự sắp xếp
          </Button>
        </Tooltip>
        <Button icon={<EyeOutlined />} onClick={() => setDslOpen(true)}>
          Xem DSL
        </Button>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} onClick={doSave}>
          Lưu
        </Button>
      </Space>

      {savedDag && (
        <Typography.Paragraph type="success" style={{ marginBottom: 8 }}>
          DAG suy được: {savedDag.length ? savedDag.join(' → ') : '(không suy được)'}
        </Typography.Paragraph>
      )}

      {/* Canvas + panel cấu hình */}
      <div style={{ flex: 1, display: 'flex', minHeight: 0, border: '1px solid #f0f0f0', borderRadius: 8 }}>
        <div style={{ flex: 1, minWidth: 0, position: 'relative' }}>
          {empty && (
            <div
              style={{
                position: 'absolute',
                inset: 0,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                zIndex: 5,
                pointerEvents: 'none',
              }}
            >
              <Empty description="Kéo thả node để dựng pipeline — bấm + Nguồn / + Biến đổi / + Xuất rồi nối chúng lại" />
            </div>
          )}
          <ReactFlow
            nodes={rfNodes}
            edges={edges}
            nodeTypes={NODE_TYPES}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onNodeClick={(_, n) => setSelectedId(n.id)}
            onPaneClick={() => setSelectedId(null)}
            deleteKeyCode={['Delete', 'Backspace']}
            fitView
            proOptions={{ hideAttribution: true }}
          >
            <Background />
            <Controls />
            <MiniMap
              nodeColor={(n) => KIND_COLOR[(n.data as AnyData).kind] ?? '#ccc'}
              pannable
              zoomable
            />
          </ReactFlow>
        </div>

        {selected && (
          <div
            style={{
              width: 340,
              borderLeft: '1px solid #f0f0f0',
              overflowY: 'auto',
              padding: 12,
              background: '#fafafa',
            }}
          >
            <ConfigPanel
              key={selected.id}
              node={selected}
              connOpts={connOpts}
              datasetNames={datasetNames}
              takenIds={takenIds}
              upstreamIds={upstreamIds}
              onChange={updateSelected}
              onDelete={() => deleteNode(selected.id)}
            />
          </div>
        )}
      </div>

      <Modal
        title="DSL sinh ra (chỉ đọc)"
        open={dslOpen}
        onCancel={() => setDslOpen(false)}
        footer={
          <Space>
            {onExportJson && (
              <Button
                onClick={() => {
                  onExportJson(currentDef())
                  setDslOpen(false)
                }}
              >
                Chép sang JSON nâng cao
              </Button>
            )}
            <Button type="primary" onClick={() => setDslOpen(false)}>
              Đóng
            </Button>
          </Space>
        }
        width={640}
      >
        <Input.TextArea
          value={JSON.stringify(currentDef(), null, 2)}
          readOnly
          rows={20}
          style={{ fontFamily: 'monospace' }}
        />
      </Modal>
    </div>
  )
}

// ---- confirm reset (409) ----
function confirmReset(impact: FlowImpact): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: 'Thay đổi cần reset state',
      content: (
        <div>
          <p>Các bước sau sẽ bị reset watermark/state:</p>
          <p>
            <b>{impact.steps_reset.join(', ') || '—'}</b>
          </p>
          {impact.datasets_orphaned.length > 0 && (
            <p>Dataset mồ côi: {impact.datasets_orphaned.join(', ')}</p>
          )}
        </div>
      ),
      okText: 'Xác nhận reset',
      okButtonProps: { danger: true },
      cancelText: 'Huỷ',
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
}

// ============================================================================
// Panel cấu hình node (Form AntD map đúng field DSL)
function ConfigPanel({
  node,
  connOpts,
  datasetNames,
  takenIds,
  upstreamIds,
  onChange,
  onDelete,
}: {
  node: FlowNode
  connOpts: { value: string; label: string }[]
  datasetNames: string[]
  takenIds: Set<string>
  upstreamIds: string[]
  onChange: (patch: Partial<AnyData>) => void
  onDelete: () => void
}) {
  const d = node.data as AnyData
  const dsOpts = datasetNames.map((n) => ({ value: n, label: n }))

  // đổi stepId: giữ duy nhất.
  const onIdChange = (raw: string) => {
    const s = slug(raw)
    const others = new Set(takenIds)
    others.delete(d.stepId)
    onChange({ stepId: others.has(s) ? d.stepId : s })
  }

  const header = (
    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
      <Tag color={KIND_COLOR[d.kind]}>{KIND_LABEL[d.kind]}</Tag>
      <Button size="small" danger icon={<DeleteOutlined />} onClick={onDelete}>
        Xoá
      </Button>
    </Space>
  )

  return (
    <Card size="small" title={header} styles={{ body: { paddingTop: 12 } }}>
      {d.errors?.length ? (
        <Card size="small" style={{ marginBottom: 12, borderColor: '#ffccc7' }}>
          {d.errors.map((e, i) => (
            <div key={i} style={{ color: '#cf1322', fontSize: 12 }}>
              {e}
            </div>
          ))}
        </Card>
      ) : null}

      <Form layout="vertical" size="small">
        <Form.Item label="ID bước (dùng làm tên bảng trong SQL)">
          <Input value={d.stepId} onChange={(e) => onIdChange(e.target.value)} />
        </Form.Item>

        {d.kind === 'source' && (
          <SourceForm d={d as SourceData} connOpts={connOpts} dsOpts={dsOpts} onChange={onChange} />
        )}
        {d.kind === 'transform' && (
          <TransformForm
            d={d as TransformData}
            dsOpts={dsOpts}
            upstreamIds={upstreamIds}
            onChange={onChange}
          />
        )}
        {d.kind === 'export' && (
          <ExportForm d={d as ExportData} connOpts={connOpts} onChange={onChange} />
        )}
      </Form>
    </Card>
  )
}

function ChipInput({
  label,
  value,
  onChange,
  placeholder,
  options,
}: {
  label: string
  value: string[]
  onChange: (v: string[]) => void
  placeholder?: string
  options?: { value: string; label: string }[]
}) {
  return (
    <Form.Item label={label}>
      <Select
        mode="tags"
        value={value}
        onChange={onChange}
        placeholder={placeholder}
        options={options}
        tokenSeparators={[',']}
        style={{ width: '100%' }}
      />
    </Form.Item>
  )
}

function SourceForm({
  d,
  connOpts,
  dsOpts,
  onChange,
}: {
  d: SourceData
  connOpts: { value: string; label: string }[]
  dsOpts: { value: string; label: string }[]
  onChange: (patch: Partial<SourceData>) => void
}) {
  const [useQuery, setUseQuery] = useState(!!d.query && !d.table)
  const isIncremental = d.mode === 'incremental_append' || d.mode === 'incremental_merge'
  const isMerge = d.mode === 'incremental_merge'
  const isSnapshot = d.mode === 'snapshot'
  return (
    <>
      <Form.Item label="Kết nối (connection)">
        <Select
          value={d.connection || undefined}
          onChange={(v) => onChange({ connection: v })}
          options={connOpts}
          placeholder="chọn kết nối"
          showSearch
          allowClear
        />
      </Form.Item>

      <Form.Item label="Nguồn dữ liệu">
        <Radio.Group
          value={useQuery ? 'query' : 'table'}
          onChange={(e) => {
            const q = e.target.value === 'query'
            setUseQuery(q)
            onChange(q ? { table: undefined } : { query: undefined })
          }}
          optionType="button"
          size="small"
          style={{ marginBottom: 8 }}
        >
          <Radio.Button value="table">Bảng</Radio.Button>
          <Radio.Button value="query">Câu SQL</Radio.Button>
        </Radio.Group>
        {useQuery ? (
          <Input.TextArea
            value={d.query}
            onChange={(e) => onChange({ query: e.target.value })}
            rows={3}
            placeholder="SELECT * FROM ..."
            style={{ fontFamily: 'monospace' }}
          />
        ) : (
          <Input
            value={d.table}
            onChange={(e) => onChange({ table: e.target.value })}
            placeholder="public.orders"
          />
        )}
      </Form.Item>

      <Form.Item label="Chế độ nạp (mode)">
        <Select
          value={d.mode}
          onChange={(v) => onChange({ mode: v })}
          options={SOURCE_MODES.map((m) => ({ value: m, label: m }))}
        />
      </Form.Item>

      {(isIncremental || (isSnapshot && d.strategy === 'timestamp')) && (
        <>
          <Form.Item label="Cursor · cột (updated_at…)">
            <Input
              value={d.cursorColumn}
              onChange={(e) => onChange({ cursorColumn: e.target.value })}
              placeholder="updated_at"
            />
          </Form.Item>
          {isIncremental && (
            <Form.Item label="Cursor · giá trị khởi tạo">
              <Input
                value={d.cursorInitial}
                onChange={(e) => onChange({ cursorInitial: e.target.value })}
                placeholder="2024-01-01"
              />
            </Form.Item>
          )}
        </>
      )}

      {(isMerge || isSnapshot) && (
        <ChipInput
          label="Primary key"
          value={d.primaryKey}
          onChange={(v) => onChange({ primaryKey: v })}
          placeholder="id"
        />
      )}

      {isMerge && (
        <>
          <Form.Item label="Chiến lược merge (strategy)">
            <Select
              value={d.strategy}
              onChange={(v) => onChange({ strategy: v })}
              options={MERGE_STRATEGIES.map((m) => ({ value: m, label: m }))}
              allowClear
              placeholder="delete_insert (mặc định)"
            />
          </Form.Item>
          <ChipInput
            label="Merge key (⊆ partition_by)"
            value={d.mergeKey}
            onChange={(v) => onChange({ mergeKey: v })}
          />
          <ChipInput
            label="Partition by (target)"
            value={d.partitionBy}
            onChange={(v) => onChange({ partitionBy: v })}
          />
          <Form.Item label="Cho phép ghi đè toàn bộ (allow_full_rewrite)">
            <Switch
              checked={!!d.allowFullRewrite}
              onChange={(v) => onChange({ allowFullRewrite: v })}
            />
          </Form.Item>
        </>
      )}

      {isSnapshot && (
        <>
          <Form.Item label="Chiến lược snapshot (strategy)">
            <Select
              value={d.strategy}
              onChange={(v) => onChange({ strategy: v })}
              options={SNAPSHOT_STRATEGIES.map((m) => ({ value: m, label: m }))}
              allowClear
              placeholder="check (mặc định)"
            />
          </Form.Item>
          <Form.Item label="Xử lý bản ghi biến mất (hard_deletes)">
            <Select
              value={d.hardDeletes}
              onChange={(v) => onChange({ hardDeletes: v })}
              options={HARD_DELETES.map((m) => ({ value: m, label: m }))}
              allowClear
            />
          </Form.Item>
          {d.strategy === 'check' && (
            <ChipInput
              label="Cột so đổi (check_columns, rỗng = tất cả)"
              value={d.checkColumns}
              onChange={(v) => onChange({ checkColumns: v })}
            />
          )}
        </>
      )}

      <Form.Item label="Target · namespace">
        <Input
          value={d.targetNs}
          onChange={(e) => onChange({ targetNs: e.target.value })}
          placeholder="raw (mặc định)"
        />
      </Form.Item>
      <Form.Item label="Target · dataset">
        <Select
          value={d.targetDataset}
          onChange={(v) => onChange({ targetDataset: v })}
          options={dsOpts}
          placeholder={`mặc định = ${d.stepId}`}
          mode="tags"
          maxCount={1}
          allowClear
        />
      </Form.Item>
    </>
  )
}

function TransformForm({
  d,
  dsOpts,
  upstreamIds,
  onChange,
}: {
  d: TransformData
  dsOpts: { value: string; label: string }[]
  upstreamIds: string[]
  onChange: (patch: Partial<TransformData>) => void
}) {
  const insertFrom = (id: string) => {
    const sql = d.sql.trim()
    const next = /from\s*$/i.test(sql) || sql === '' ? `${sql} ${id}`.trim() : `${sql}\n-- FROM ${id}`
    onChange({ sql: next })
  }
  return (
    <>
      <Form.Item label="Loại biến đổi (kind)">
        <Select
          value={d.transformKind}
          onChange={(v) => onChange({ transformKind: v })}
          options={TRANSFORM_KINDS.map((m) => ({ value: m, label: m }))}
        />
      </Form.Item>

      {upstreamIds.length > 0 && (
        <Form.Item label="Bảng nguồn đã nối (bấm để chèn vào FROM)">
          <Space wrap size={4}>
            {upstreamIds.map((id) => (
              <Tag
                key={id}
                color="green"
                style={{ cursor: 'pointer' }}
                onClick={() => insertFrom(id)}
              >
                {id}
              </Tag>
            ))}
          </Space>
        </Form.Item>
      )}

      <Form.Item label="SQL (SELECT-only)">
        <Input.TextArea
          value={d.sql}
          onChange={(e) => onChange({ sql: e.target.value })}
          rows={6}
          style={{ fontFamily: 'monospace' }}
          placeholder="SELECT ... FROM <id_nguon>"
        />
      </Form.Item>

      {d.transformKind === 'incremental_by_time' && (
        <>
          <Form.Item label="Cột thời gian (time_column)">
            <Input
              value={d.timeColumn}
              onChange={(e) => onChange({ timeColumn: e.target.value })}
              placeholder="event_day"
            />
          </Form.Item>
          <Form.Item label="Interval">
            <Select
              value={d.interval}
              onChange={(v) => onChange({ interval: v })}
              options={INTERVALS.map((m) => ({ value: m, label: m }))}
            />
          </Form.Item>
          <Form.Item label="Lookback (số kỳ đọc lùi)">
            <InputNumber
              value={d.lookback}
              onChange={(v) => onChange({ lookback: v ?? undefined })}
              min={0}
              style={{ width: '100%' }}
            />
          </Form.Item>
        </>
      )}

      <Form.Item label="Target · namespace">
        <Input
          value={d.targetNs}
          onChange={(e) => onChange({ targetNs: e.target.value })}
          placeholder="marts (mặc định)"
        />
      </Form.Item>
      <Form.Item label="Target · dataset">
        <Select
          value={d.targetDataset}
          onChange={(v) => onChange({ targetDataset: v })}
          options={dsOpts}
          placeholder={`mặc định = ${d.stepId}`}
          mode="tags"
          maxCount={1}
          allowClear
        />
      </Form.Item>
    </>
  )
}

function ExportForm({
  d,
  connOpts,
  onChange,
}: {
  d: ExportData
  connOpts: { value: string; label: string }[]
  onChange: (patch: Partial<ExportData>) => void
}) {
  return (
    <>
      <Form.Item label="Đích xuất">
        <Radio.Group
          value={d.dest}
          onChange={(e) => onChange({ dest: e.target.value })}
          optionType="button"
          size="small"
        >
          <Radio.Button value="format">Tệp (csv/json/parquet)</Radio.Button>
          <Radio.Button value="connection">Bảng DB</Radio.Button>
        </Radio.Group>
      </Form.Item>

      {d.dest === 'format' ? (
        <Form.Item label="Định dạng tệp">
          <Select
            value={d.format}
            onChange={(v) => onChange({ format: v })}
            options={['csv', 'json', 'parquet'].map((m) => ({ value: m, label: m }))}
          />
        </Form.Item>
      ) : (
        <>
          <Form.Item label="Kết nối đích">
            <Select
              value={d.connection || undefined}
              onChange={(v) => onChange({ connection: v })}
              options={connOpts}
              showSearch
              allowClear
              placeholder="chọn kết nối"
            />
          </Form.Item>
          <Form.Item label="Bảng đích">
            <Input
              value={d.table}
              onChange={(e) => onChange({ table: e.target.value })}
              placeholder="public.orders_out"
            />
          </Form.Item>
        </>
      )}

      <Form.Item label="Chế độ ghi (mode)">
        <Select
          value={d.mode}
          onChange={(v) => onChange({ mode: v })}
          options={EXPORT_MODES.map((m) => ({ value: m, label: m }))}
        />
      </Form.Item>

      {d.mode === 'upsert' && (
        <ChipInput
          label="Khoá upsert (keys)"
          value={d.keys}
          onChange={(v) => onChange({ keys: v })}
          placeholder="id"
        />
      )}

      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        Nối một node Nguồn/Biến đổi vào node Xuất này để đặt <code>input</code>.
      </Typography.Paragraph>
    </>
  )
}
