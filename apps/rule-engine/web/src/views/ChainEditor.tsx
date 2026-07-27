// Canvas editor for one chain: palette → nodes → ports → edges, plus live
// trace over SSE.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  Background,
  Controls,
  MarkerType,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  type Connection,
  type Edge as RFEdge,
  type EdgeChange,
  type NodeChange,
  type NodeTypes,
} from '@xyflow/react'
import {
  Alert,
  App as AntApp,
  Button,
  Flex,
  Input,
  Modal,
  Select,
  Space,
  Spin,
  Switch,
  Tag,
  Tooltip,
  Typography,
  theme,
} from 'antd'
import {
  ArrowLeftOutlined,
  BugOutlined,
  CheckCircleOutlined,
  PlayCircleOutlined,
  PoweroffOutlined,
  SaveOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import { api, apiUrl, type GraphNodeDto } from '../api'
import Palette, { DRAG_MIME } from '../components/Palette'
import NodeDrawer from '../components/NodeDrawer'
import TracePanel from '../components/TracePanel'
import RuleNode, { NODE_W, type RuleFlowNode, type RuleNodeData } from '../components/RuleNode'
import { portsOf } from '../ports'
import {
  DEFAULT_OPTS,
  type Chain,
  type ChainEdge,
  type ChainNode,
  type ChainStatus,
  type EngineEvent,
  type EvHop,
  type HopRow,
  type Issue,
  type JsonObject,
  type JsonSchema,
  type LogRow,
  type RuleSpec,
  type RunRow,
  type TraceHop,
} from '../types'

const nodeTypes: NodeTypes = { rule: RuleNode }

const STATUS_TAG: Record<ChainStatus, string> = {
  ACTIVE: 'green',
  INACTIVE: 'default',
  ERROR: 'red',
}

const TOOLBAR_H = 56

function defaultConfig(schema: JsonSchema | undefined): JsonObject {
  const out: JsonObject = {}
  for (const [k, v] of Object.entries(schema?.properties ?? {})) {
    if (v.default !== undefined) out[k] = v.default
  }
  return out
}

function toFlowNode(n: ChainNode, spec?: RuleSpec): RuleFlowNode {
  return {
    id: n.id,
    type: 'rule',
    position: { x: n.x ?? 0, y: n.y ?? 0 },
    data: {
      ruleId: n.rule,
      name: n.name || n.id,
      config: (n.config ?? {}) as JsonObject,
      opts: { ...DEFAULT_OPTS, ...(n.opts ?? {}) },
      debug: Boolean(n.debug),
      spec,
      errors: [],
      warnings: [],
      flash: false,
    },
  }
}

function toFlowEdge(e: ChainEdge): RFEdge {
  return {
    id: e.id,
    source: e.from.node,
    sourceHandle: e.from.port,
    target: e.to.node,
    targetHandle: e.to.port,
    markerEnd: { type: MarkerType.ArrowClosed },
  }
}

const hopFromRow = (h: HopRow): TraceHop => ({
  key: `r${h.run_id}-${h.seq}-${h.id}`,
  runId: h.run_id,
  seq: h.seq,
  node: h.node,
  rule: h.rule,
  inPort: h.in_port,
  outPort: h.out_port,
  kind: h.kind,
  error: h.error ?? '',
  durMs: h.dur_ms,
  ts: h.ts,
  data: h.data ?? '',
})

const hopFromEvent = (e: EvHop, i: number): TraceHop => ({
  key: `l${e.runId}-${e.seq}-${i}`,
  runId: e.runId,
  seq: e.seq,
  node: e.node,
  rule: e.rule,
  inPort: e.inPort,
  outPort: e.outPort,
  kind: e.kind,
  error: e.error ?? '',
  durMs: e.durMs,
  ts: Date.now(),
  data: typeof e.data === 'string' ? e.data : JSON.stringify(e.data, null, 2),
})

export default function ChainEditor(props: { chainId: number; onBack: () => void }) {
  return (
    <ReactFlowProvider>
      <Editor {...props} />
    </ReactFlowProvider>
  )
}

function Editor({ chainId, onBack }: { chainId: number; onBack: () => void }) {
  const { message, modal } = AntApp.useApp()
  const { token } = theme.useToken()
  const { screenToFlowPosition } = useReactFlow()
  const wrapper = useRef<HTMLDivElement | null>(null)

  const [specs, setSpecs] = useState<RuleSpec[]>([])
  const specMap = useMemo(() => new Map(specs.map((s) => [s.id, s])), [specs])
  const specMapRef = useRef(specMap)
  specMapRef.current = specMap

  const [chain, setChain] = useState<Chain | null>(null)
  const [nodes, setNodes, onNodesChange] = useNodesState<RuleFlowNode>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<RFEdge>([])
  const [issues, setIssues] = useState<Issue[]>([])
  const [loading, setLoading] = useState(true)
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const [busy, setBusy] = useState(false)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [hideWarnings, setHideWarnings] = useState(false)

  // trace / logs
  const [traceOpen, setTraceOpen] = useState(false)
  const [runs, setRuns] = useState<RunRow[]>([])
  const [runId, setRunId] = useState<number | null>(null)
  const [hops, setHops] = useState<TraceHop[]>([])
  const [logs, setLogs] = useState<LogRow[]>([])

  // trigger modal
  const [triggerOpen, setTriggerOpen] = useState(false)
  const [triggerNode, setTriggerNode] = useState<string | undefined>()
  const [triggerJson, setTriggerJson] = useState('{}')

  const nodesRef = useRef(nodes)
  nodesRef.current = nodes
  const edgesRef = useRef(edges)
  edgesRef.current = edges
  const runIdRef = useRef<number | null>(runId)
  runIdRef.current = runId

  const fail = useCallback(
    (e: unknown) => message.error(e instanceof Error ? e.message : String(e)),
    [message],
  )

  // ------------------------------------------------------------- bootstrap

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    ;(async () => {
      try {
        const rules = await api.registry()
        if (cancelled) return
        setSpecs(rules)
        const map = new Map(rules.map((r) => [r.id, r]))
        const res = await api.getChain(chainId)
        if (cancelled) return
        setChain({ ...res.chain, deployed: res.deployed })
        setNodes(res.nodes.map((n) => toFlowNode(n, map.get(n.rule))))
        setEdges(res.edges.map(toFlowEdge))
        setIssues(res.issues ?? [])
        setDirty(false)
        const [r, l] = await Promise.all([
          api.runs(chainId).catch(() => [] as RunRow[]),
          api.logs(chainId).catch(() => [] as LogRow[]),
        ])
        if (cancelled) return
        setRuns(r)
        setLogs(l)
      } catch (e) {
        if (!cancelled) fail(e)
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [chainId, setNodes, setEdges, fail])

  // Issues drive the red borders / badges on nodes.
  useEffect(() => {
    setNodes((ns) =>
      ns.map((n) => {
        const errs = issues
          .filter((i) => i.level === 'error' && i.node === n.id)
          .map((i) => i.message)
        const warns = issues
          .filter((i) => i.level === 'warning' && i.node === n.id)
          .map((i) => i.message)
        if (
          errs.join('|') === n.data.errors.join('|') &&
          warns.join('|') === n.data.warnings.join('|')
        ) {
          return n
        }
        return { ...n, data: { ...n.data, errors: errs, warnings: warns } }
      }),
    )
  }, [issues, setNodes])

  // ------------------------------------------------------------------- SSE

  const flashNode = useCallback(
    (id: string) => {
      setNodes((ns) => ns.map((n) => (n.id === id ? { ...n, data: { ...n.data, flash: true } } : n)))
      window.setTimeout(() => {
        setNodes((ns) =>
          ns.map((n) => (n.id === id ? { ...n, data: { ...n.data, flash: false } } : n)),
        )
      }, 800)
    },
    [setNodes],
  )

  const pulseEdges = useCallback(
    (node: string, port: string) => {
      setEdges((es) =>
        es.map((e) =>
          e.source === node && (e.sourceHandle ?? 'out') === port ? { ...e, animated: true } : e,
        ),
      )
      window.setTimeout(
        () => setEdges((es) => es.map((e) => (e.animated ? { ...e, animated: false } : e))),
        900,
      )
    },
    [setEdges],
  )

  const liveSeq = useRef(0)

  const handleEvent = useCallback(
    (ev: EngineEvent) => {
      if (ev.chainId !== chainId) return
      switch (ev.type) {
        case 'runStart':
          setRunId(ev.runId)
          setHops([])
          liveSeq.current = 0
          void api.runs(chainId).then(setRuns).catch(() => undefined)
          break
        case 'hop': {
          flashNode(ev.node)
          if (ev.outPort) pulseEdges(ev.node, ev.outPort)
          if (runIdRef.current === null || runIdRef.current === ev.runId) {
            const hop = hopFromEvent(ev, liveSeq.current++)
            setHops((h) => [...h, hop].slice(-500))
          }
          break
        }
        case 'runEnd':
          void api.runs(chainId).then(setRuns).catch(() => undefined)
          // Reconcile against the stored trace. Live hops can be lost to a
          // race (a run that finishes before `trigger` even returns), and the
          // server copy is the one the user can come back to later.
          void api
            .hops(ev.runId)
            .then((rows) => {
              if (runIdRef.current === ev.runId && rows.length) {
                setHops(rows.map(hopFromRow))
              }
            })
            .catch(() => undefined)
          if (ev.error) message.error(`Run #${ev.runId} lỗi: ${ev.error}`)
          break
        case 'log':
          setLogs((l) =>
            [
              {
                id: 0,
                chain_id: ev.chainId,
                run_id: ev.runId,
                level: ev.level,
                node: ev.node,
                message: ev.message,
                ts: ev.ts,
              } as LogRow,
              ...l,
            ].slice(0, 400),
          )
          break
        case 'chainStatus':
          setChain((c) => (c ? { ...c, status: ev.status as ChainStatus } : c))
          if (ev.error) message.error(ev.error)
          break
      }
    },
    [chainId, flashNode, pulseEdges, message],
  )

  const handlerRef = useRef(handleEvent)
  handlerRef.current = handleEvent

  useEffect(() => {
    let es: EventSource | null = null
    let timer: number | undefined
    let closed = false

    const connect = () => {
      if (closed) return
      es = new EventSource(apiUrl('events'))
      es.addEventListener('engine', (e) => {
        try {
          handlerRef.current(JSON.parse((e as MessageEvent).data) as EngineEvent)
        } catch {
          /* ignore malformed frames */
        }
      })
      es.onerror = () => {
        if (closed) return
        if (es && es.readyState === EventSource.CLOSED) {
          es.close()
          es = null
          timer = window.setTimeout(connect, 2000)
        }
      }
    }
    connect()

    return () => {
      closed = true
      if (timer) window.clearTimeout(timer)
      es?.close()
    }
  }, [])

  // ------------------------------------------------------------ graph edit

  const uniqueId = useCallback((ruleId: string) => {
    const taken = new Set(nodesRef.current.map((n) => n.id))
    let i = 1
    let id = `${ruleId}_${i}`
    while (taken.has(id)) id = `${ruleId}_${++i}`
    return id
  }, [])

  const addNode = useCallback(
    (spec: RuleSpec, position: { x: number; y: number }) => {
      const id = uniqueId(spec.id)
      // `join` / `merge` only barrier when opts.join is set; the default 'any'
      // would run the node once per incoming edge. Pick it from the rule here so
      // dragging one of these nodes out already wires the barrier correctly.
      const join =
        spec.id === 'join' ? 'all' : spec.id === 'merge' ? 'merge' : DEFAULT_OPTS.join
      const data: RuleNodeData = {
        ruleId: spec.id,
        name: spec.name,
        config: defaultConfig(spec.config_schema),
        opts: { ...DEFAULT_OPTS, join },
        debug: false,
        spec,
        errors: [],
        warnings: [],
        flash: false,
      }
      setNodes((ns) => [...ns, { id, type: 'rule', position, data }])
      setSelectedId(id)
      setDirty(true)
    },
    [setNodes, uniqueId],
  )

  const addToCenter = useCallback(
    (spec: RuleSpec) => {
      const box = wrapper.current?.getBoundingClientRect()
      const point = box
        ? screenToFlowPosition({ x: box.x + box.width / 2, y: box.y + box.height / 2 })
        : { x: 120, y: 120 }
      addNode(spec, { x: point.x - NODE_W / 2, y: point.y - 40 })
    },
    [addNode, screenToFlowPosition],
  )

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      const ruleId = e.dataTransfer.getData(DRAG_MIME)
      if (!ruleId) return
      const spec = specMapRef.current.get(ruleId)
      if (!spec) {
        message.warning(`Không tìm thấy loại node \`${ruleId}\` trong registry.`)
        return
      }
      const point = screenToFlowPosition({ x: e.clientX, y: e.clientY })
      addNode(spec, { x: point.x - NODE_W / 2, y: point.y - 20 })
    },
    [addNode, screenToFlowPosition, message],
  )

  /** Returns a Vietnamese reason when the connection must be refused. */
  const rejectReason = useCallback((c: Connection | RFEdge): string | null => {
    const { source, target, sourceHandle, targetHandle } = c
    if (!source || !target) return 'Thiếu đầu nối.'
    if (!sourceHandle || !targetHandle) return 'Phải kéo từ một cổng ra sang một cổng vào.'

    const src = nodesRef.current.find((n) => n.id === source)
    const dst = nodesRef.current.find((n) => n.id === target)
    if (!src || !dst) return 'Không tìm thấy node.'

    if (dst.data.spec?.isSource) {
      return `\`${dst.data.name}\` là node nguồn — nó không nhận cổng vào.`
    }

    const srcPorts = portsOf(src.data.spec, src.data.config)
    const dstPorts = portsOf(dst.data.spec, dst.data.config)

    const outPort = srcPorts.outputs.find((p) => p.id === sourceHandle)
    if (!outPort) return `Node \`${src.data.name}\` không có cổng ra \`${sourceHandle}\`.`
    const inPort = dstPorts.inputs.find((p) => p.id === targetHandle)
    if (!inPort) return `Node \`${dst.data.name}\` không có cổng vào \`${targetHandle}\`.`

    const dup = edgesRef.current.some(
      (e) =>
        e.source === source &&
        (e.sourceHandle ?? '') === sourceHandle &&
        e.target === target &&
        (e.targetHandle ?? '') === targetHandle,
    )
    if (dup) return 'Cạnh này đã tồn tại.'

    if (outPort.arity === 'one') {
      const used = edgesRef.current.some(
        (e) => e.source === source && (e.sourceHandle ?? '') === sourceHandle,
      )
      if (used) return `Cổng \`${outPort.label}\` chỉ nhận 1 cạnh. Xoá cạnh cũ trước.`
    }
    return null
  }, [])

  const lastReject = useRef('')
  const connected = useRef(false)

  const isValidConnection = useCallback(
    (c: Connection | RFEdge) => {
      const reason = rejectReason(c)
      if (reason) {
        lastReject.current = reason
        return false
      }
      return true
    },
    [rejectReason],
  )

  const onConnect = useCallback(
    (c: Connection) => {
      const reason = rejectReason(c)
      if (reason) {
        message.warning(reason)
        return
      }
      connected.current = true
      const id = `e_${Date.now().toString(36)}_${Math.floor(Math.random() * 1e4)}`
      setEdges((es) => [
        ...es,
        {
          id,
          source: c.source,
          sourceHandle: c.sourceHandle,
          target: c.target,
          targetHandle: c.targetHandle,
          markerEnd: { type: MarkerType.ArrowClosed },
        },
      ])
      setDirty(true)
    },
    [rejectReason, setEdges, message],
  )

  const handleNodesChange = useCallback(
    (changes: NodeChange<RuleFlowNode>[]) => {
      onNodesChange(changes)
      if (changes.some((c) => c.type === 'position' || c.type === 'remove' || c.type === 'add')) {
        setDirty(true)
      }
    },
    [onNodesChange],
  )

  const handleEdgesChange = useCallback(
    (changes: EdgeChange<RFEdge>[]) => {
      onEdgesChange(changes)
      if (changes.some((c) => c.type === 'remove' || c.type === 'add')) setDirty(true)
    },
    [onEdgesChange],
  )

  const patchNode = useCallback(
    (id: string, patch: Partial<RuleNodeData>) => {
      setNodes((ns) => ns.map((n) => (n.id === id ? { ...n, data: { ...n.data, ...patch } } : n)))
      setDirty(true)
    },
    [setNodes],
  )

  const deleteNode = useCallback(
    (id: string) => {
      setNodes((ns) => ns.filter((n) => n.id !== id))
      setEdges((es) => es.filter((e) => e.source !== id && e.target !== id))
      setSelectedId(null)
      setDirty(true)
    },
    [setNodes, setEdges],
  )

  const deleteEdge = useCallback(
    (id: string) => {
      setEdges((es) => es.filter((e) => e.id !== id))
      setDirty(true)
    },
    [setEdges],
  )

  // ------------------------------------------------------------- toolbar ops

  const save = useCallback(async () => {
    setSaving(true)
    try {
      const dtoNodes: GraphNodeDto[] = nodesRef.current.map((n) => ({
        id: n.id,
        rule: n.data.ruleId,
        name: n.data.name,
        config: n.data.config ?? {},
        opts: { ...DEFAULT_OPTS, ...n.data.opts },
        x: Math.round(n.position.x),
        y: Math.round(n.position.y),
        debug: n.data.debug,
      }))
      const dtoEdges: ChainEdge[] = edgesRef.current.map((e) => ({
        id: e.id,
        from: { node: e.source, port: e.sourceHandle ?? 'out' },
        to: { node: e.target, port: e.targetHandle ?? 'in' },
      }))
      const res = await api.putGraph(chainId, dtoNodes, dtoEdges)
      setIssues(res.issues ?? [])
      setDirty(false)
      setHideWarnings(false)
      const errCount = (res.issues ?? []).filter((i) => i.level === 'error').length
      if (errCount > 0) {
        message.error(`Đã lưu bản nháp, nhưng còn ${errCount} lỗi phải sửa trước khi kích hoạt.`)
      } else if (res.redeployed) {
        message.success('Đã lưu và nạp lại luồng đang chạy.')
      } else {
        message.success('Đã lưu.')
      }
    } catch (e) {
      fail(e)
    } finally {
      setSaving(false)
    }
  }, [chainId, message, fail])

  const validate = useCallback(async () => {
    setBusy(true)
    try {
      const res = await api.validate(chainId)
      setIssues(res.issues ?? [])
      setHideWarnings(false)
      const errs = (res.issues ?? []).filter((i) => i.level === 'error')
      if (errs.length === 0) message.success('Đồ thị hợp lệ.')
      else message.error(`${errs.length} lỗi: ${errs[0].message}`)
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }, [chainId, message, fail])

  const reloadChain = useCallback(async () => {
    try {
      const res = await api.getChain(chainId)
      setChain({ ...res.chain, deployed: res.deployed })
      setIssues(res.issues ?? [])
    } catch (e) {
      fail(e)
    }
  }, [chainId, fail])

  const activate = useCallback(async () => {
    if (dirty) {
      message.warning('Lưu đồ thị trước khi kích hoạt.')
      return
    }
    setBusy(true)
    try {
      const res = await api.activate(chainId)
      setIssues(res.issues ?? [])
      message.success('Đã kích hoạt luồng.')
      await reloadChain()
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }, [chainId, dirty, message, reloadChain, fail])

  const deactivate = useCallback(async () => {
    setBusy(true)
    try {
      await api.deactivate(chainId)
      message.success('Đã dừng luồng.')
      await reloadChain()
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }, [chainId, message, reloadChain, fail])

  const setDebug = useCallback(
    async (v: boolean) => {
      setChain((c) => (c ? { ...c, debug: v } : c))
      try {
        await api.patchChain(chainId, { debug: v })
      } catch (e) {
        setChain((c) => (c ? { ...c, debug: !v } : c))
        fail(e)
      }
    },
    [chainId, fail],
  )

  const sourceNodes = useMemo(() => nodes.filter((n) => n.data.spec?.isSource), [nodes])

  const openTrigger = useCallback(() => {
    if (chain?.status !== 'ACTIVE') {
      message.warning('Luồng chưa chạy. Bấm Kích hoạt trước khi bơm sự kiện thử.')
      return
    }
    const manual = sourceNodes.find((n) => n.data.ruleId === 'manual') ?? sourceNodes[0]
    setTriggerNode(manual?.id)
    setTriggerOpen(true)
  }, [chain?.status, sourceNodes, message])

  const doTrigger = useCallback(async () => {
    let data: unknown = {}
    try {
      data = triggerJson.trim() === '' ? {} : JSON.parse(triggerJson)
    } catch (e) {
      message.error(`Payload không phải JSON hợp lệ: ${e instanceof Error ? e.message : e}`)
      return
    }
    try {
      // Clear BEFORE the request: a fast chain finishes and streams its whole
      // trace over SSE before this await resolves, and clearing afterwards
      // would wipe exactly those hops.
      setHops([])
      liveSeq.current = 0
      const id = await api.trigger(chainId, { node: triggerNode, data })
      setTriggerOpen(false)
      setRunId(id)
      setTraceOpen(true)
      message.success(`Đã bơm sự kiện — run #${id}.`)
      void api.runs(chainId).then(setRuns).catch(() => undefined)
    } catch (e) {
      fail(e)
    }
  }, [chainId, triggerJson, triggerNode, message, fail])

  const selectRun = useCallback(
    async (id: number) => {
      setRunId(id)
      try {
        const rows = await api.hops(id)
        setHops(rows.map(hopFromRow))
      } catch (e) {
        fail(e)
      }
    },
    [fail],
  )

  const nodeLabel = useCallback(
    (id: string) => nodesRef.current.find((n) => n.id === id)?.data.name ?? id,
    [],
  )

  const clearState = useCallback(() => {
    modal.confirm({
      title: 'Xoá state của luồng?',
      content: 'Bộ nhớ của các node có trạng thái (moving-average, kalman…) sẽ về 0.',
      okText: 'Xoá',
      cancelText: 'Huỷ',
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await api.clearState(chainId)
          message.success('Đã xoá state.')
        } catch (e) {
          fail(e)
        }
      },
    })
  }, [chainId, modal, message, fail])

  // ------------------------------------------------------------------ render

  const selected = nodes.find((n) => n.id === selectedId) ?? null
  const warnings = issues.filter((i) => i.level === 'warning')
  const errors = issues.filter((i) => i.level === 'error')

  if (loading) {
    return (
      <Flex align="center" justify="center" style={{ height: '100vh' }}>
        <Spin size="large" description="Đang tải luồng…">
          <div style={{ padding: 40 }} />
        </Spin>
      </Flex>
    )
  }

  return (
    <div style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}>
      <Flex
        align="center"
        gap={10}
        style={{
          height: TOOLBAR_H,
          flex: 'none',
          padding: '0 12px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorBgContainer,
          overflowX: 'auto',
        }}
      >
        <Button icon={<ArrowLeftOutlined />} onClick={onBack}>
          Quay lại
        </Button>
        <Typography.Text strong ellipsis style={{ maxWidth: 260 }}>
          {chain?.name ?? `#${chainId}`}
        </Typography.Text>
        <Tag color={STATUS_TAG[chain?.status ?? 'INACTIVE']}>{chain?.status ?? '—'}</Tag>
        {chain?.deployed && <Tag color="processing">● đang chạy</Tag>}
        {dirty && <Tag color="warning">chưa lưu</Tag>}

        <div style={{ flex: 1 }} />

        <Tooltip title="Ghi trace từng bước cho mọi node trong luồng">
          <Space size={6}>
            <BugOutlined />
            <span style={{ fontSize: 12 }}>Debug</span>
            <Switch
              size="small"
              checked={Boolean(chain?.debug)}
              onChange={(v) => void setDebug(v)}
            />
          </Space>
        </Tooltip>

        <Button icon={<SaveOutlined />} type="primary" loading={saving} onClick={() => void save()}>
          Lưu
        </Button>
        {chain?.status === 'ACTIVE' ? (
          <Button icon={<PoweroffOutlined />} danger loading={busy} onClick={() => void deactivate()}>
            Dừng
          </Button>
        ) : (
          <Button
            icon={<PlayCircleOutlined />}
            loading={busy}
            onClick={() => void activate()}
          >
            Kích hoạt
          </Button>
        )}
        <Button icon={<ThunderboltOutlined />} onClick={openTrigger}>
          Chạy thử
        </Button>
        <Button icon={<CheckCircleOutlined />} loading={busy} onClick={() => void validate()}>
          Kiểm tra
        </Button>
        <Button onClick={() => setTraceOpen(true)}>Log</Button>
        <Button type="text" onClick={clearState}>
          Xoá state
        </Button>
      </Flex>

      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <Palette rules={specs} onAdd={addToCenter} />

        <div
          ref={wrapper}
          style={{ flex: 1, position: 'relative', height: `calc(100vh - ${TOOLBAR_H}px)` }}
          onDrop={onDrop}
          onDragOver={(e) => {
            e.preventDefault()
            e.dataTransfer.dropEffect = 'move'
          }}
        >
          {errors.length > 0 && (
            <Alert
              className="canvas-alert"
              type="error"
              showIcon
              title={`${errors.length} lỗi cần sửa`}
              description={
                <ul style={{ margin: 0, paddingLeft: 18 }}>
                  {errors.slice(0, 4).map((i, k) => (
                    <li key={k}>
                      {i.node ? `[${nodeLabel(i.node)}] ` : ''}
                      {i.message}
                    </li>
                  ))}
                </ul>
              }
            />
          )}
          {errors.length === 0 && warnings.length > 0 && !hideWarnings && (
            <Alert
              className="canvas-alert"
              type="warning"
              showIcon
              closable
              onClose={() => setHideWarnings(true)}
              title={`${warnings.length} cảnh báo`}
              description={
                <ul style={{ margin: 0, paddingLeft: 18 }}>
                  {warnings.slice(0, 4).map((i, k) => (
                    <li key={k}>
                      {i.node ? `[${nodeLabel(i.node)}] ` : ''}
                      {i.message}
                    </li>
                  ))}
                </ul>
              }
            />
          )}

          <ReactFlow<RuleFlowNode, RFEdge>
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={handleNodesChange}
            onEdgesChange={handleEdgesChange}
            onConnect={onConnect}
            onConnectStart={() => {
              lastReject.current = ''
              connected.current = false
            }}
            onConnectEnd={() => {
              if (!connected.current && lastReject.current) {
                message.warning(lastReject.current)
                lastReject.current = ''
              }
            }}
            isValidConnection={isValidConnection}
            onNodeClick={(_, n) => setSelectedId(n.id)}
            onPaneClick={() => setSelectedId(null)}
            deleteKeyCode={['Delete']}
            fitView
            minZoom={0.2}
            proOptions={{ hideAttribution: true }}
            defaultEdgeOptions={{ markerEnd: { type: MarkerType.ArrowClosed } }}
          >
            <Background gap={18} />
            <Controls showInteractive={false} />
            <MiniMap
              pannable
              zoomable
              nodeStrokeWidth={2}
              nodeColor={(n) => (n as RuleFlowNode).data?.spec?.color ?? '#8c8c8c'}
              style={{ height: 90, width: 140 }}
            />
          </ReactFlow>
        </div>
      </div>

      <NodeDrawer
        node={selected}
        nodes={nodes}
        edges={edges}
        onClose={() => setSelectedId(null)}
        onPatch={(patch) => selectedId && patchNode(selectedId, patch)}
        onDelete={() => selectedId && deleteNode(selectedId)}
        onDeleteEdge={deleteEdge}
      />

      <TracePanel
        open={traceOpen}
        onClose={() => setTraceOpen(false)}
        chainDebug={Boolean(chain?.debug)}
        runs={runs}
        runId={runId}
        onSelectRun={(id) => void selectRun(id)}
        hops={hops}
        logs={logs}
        onReloadRuns={() => void api.runs(chainId).then(setRuns).catch(fail)}
        onReloadLogs={() => void api.logs(chainId).then(setLogs).catch(fail)}
        nodeLabel={nodeLabel}
      />

      <Modal
        open={triggerOpen}
        title="Chạy thử"
        okText="Bơm sự kiện"
        cancelText="Huỷ"
        onCancel={() => setTriggerOpen(false)}
        onOk={() => void doTrigger()}
      >
        <Space direction="vertical" style={{ width: '100%' }} size={12}>
          <div>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Node nguồn
            </Typography.Text>
            <Select
              style={{ width: '100%' }}
              placeholder="Chọn node nguồn"
              value={triggerNode}
              onChange={setTriggerNode}
              options={sourceNodes.map((n) => ({
                value: n.id,
                label: `${n.data.spec?.icon ?? ''} ${n.data.name} (${n.data.ruleId})`,
              }))}
              notFoundContent="Luồng chưa có node nguồn nào"
            />
          </div>
          <div>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Payload JSON
            </Typography.Text>
            <Input.TextArea
              className="mono"
              rows={8}
              value={triggerJson}
              onChange={(e) => setTriggerJson(e.target.value)}
            />
          </div>
        </Space>
      </Modal>
    </div>
  )
}
