import { useEffect, useMemo, useRef, useState } from 'react'
import { Button, Card, Drawer, Input, Modal, Segmented, Select, Space, Spin, Tag } from 'antd'
import {
  api,
  type Customer,
  type GraphNode,
  type Relationship,
} from '../api'
import { REL_ORDER, ROLE_ORDER, roleMeta } from '../constants'
import { tk, type T } from '../i18n'
import { Avatar } from '../components/Avatar'

/// Every piece of Network state we persist between page switches AND across
/// reloads (via `/state/graph`). Lives in App scope so switching pages doesn't
/// unmount and lose it.
export type NetState = {
  nameFilter: string
  roleFilter: string[]
  kindFilter: string[]
  focus: { id: number; hops: number } | null
  pathState: { from?: number; to?: number; ids: number[]; hops: number } | null
  common: {
    focus_id: number
    themes: Array<{ theme: string; why: string; customer_ids: number[] }>
    highlight_ids: number[]
  } | null
  aiPath: {
    from: number
    to: number
    summary: string
    connections: Array<{ type: string; detail: string; strength: string }>
    bfs_path_names: string[] | null
  } | null
}

export const NET_DEFAULT: NetState = {
  nameFilter: '',
  roleFilter: [],
  kindFilter: [],
  focus: null,
  pathState: null,
  common: null,
  aiPath: null,
}

export function NetworkPage({
  net,
  setNet,
  busy,
  setBusy,
  t,
  onBackgroundResult,
  onPickCustomer,
}: {
  net: NetState
  setNet: (updater: (s: NetState) => NetState) => void
  busy: null | 'common' | 'ai_path'
  setBusy: (b: null | 'common' | 'ai_path') => void
  t: T
  onBackgroundResult: (msg: string) => void
  onPickCustomer: (id: number) => void
}) {
  const [allNodes, setAllNodes] = useState<GraphNode[]>([])
  const [allEdges, setAllEdges] = useState<Relationship[]>([])
  const [positions, setPositions] = useState<Record<number, { x: number; y: number }>>({})
  const [dragging, setDragging] = useState<number | null>(null)
  const [hovered, setHovered] = useState<number | null>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const width = 900
  const height = 560

  const { nameFilter, roleFilter, kindFilter, focus, pathState, common, aiPath } = net
  const setNameFilter = (v: string) => setNet((s) => ({ ...s, nameFilter: v }))
  const setRoleFilter = (v: string[]) => setNet((s) => ({ ...s, roleFilter: v }))
  const setKindFilter = (v: string[]) => setNet((s) => ({ ...s, kindFilter: v }))
  const setFocus = (v: NetState['focus']) => setNet((s) => ({ ...s, focus: v }))
  const setPathState = (v: NetState['pathState']) => setNet((s) => ({ ...s, pathState: v }))
  const setCommon = (v: NetState['common']) => setNet((s) => ({ ...s, common: v }))
  const setAiPath = (v: NetState['aiPath']) => setNet((s) => ({ ...s, aiPath: v }))

  const [drawerId, setDrawerId] = useState<number | null>(null)
  const [pathModal, setPathModal] = useState(false)

  // Filter graph by name/role/kind + focus subgraph.
  const { nodes, edges } = useMemo(() => {
    let ns = allNodes
    let es = allEdges
    if (focus) {
      // BFS on the client so filters compose with focus.
      const adj = new Map<number, number[]>()
      for (const e of allEdges) {
        if (!adj.has(e.from_id)) adj.set(e.from_id, [])
        if (!adj.has(e.to_id)) adj.set(e.to_id, [])
        adj.get(e.from_id)!.push(e.to_id)
        adj.get(e.to_id)!.push(e.from_id)
      }
      const seen = new Set<number>([focus.id])
      let frontier = [focus.id]
      for (let i = 0; i < focus.hops; i++) {
        const next: number[] = []
        for (const v of frontier) {
          for (const n of adj.get(v) ?? [])
            if (!seen.has(n)) {
              seen.add(n)
              next.push(n)
            }
        }
        frontier = next
      }
      ns = allNodes.filter((n) => seen.has(n.id))
      es = allEdges.filter((e) => seen.has(e.from_id) && seen.has(e.to_id))
    }
    if (roleFilter.length > 0) ns = ns.filter((n) => roleFilter.includes(n.role))
    if (nameFilter.trim()) {
      const q = nameFilter.trim().toLowerCase()
      ns = ns.filter((n) => n.name.toLowerCase().includes(q) || n.company.toLowerCase().includes(q))
    }
    const idSet = new Set(ns.map((n) => n.id))
    es = es.filter((e) => idSet.has(e.from_id) && idSet.has(e.to_id))
    if (kindFilter.length > 0) es = es.filter((e) => kindFilter.includes(e.kind))
    return { nodes: ns, edges: es }
  }, [allNodes, allEdges, focus, roleFilter, kindFilter, nameFilter])

  const pathEdgeSet = useMemo(() => {
    const s = new Set<string>()
    if (pathState?.ids && pathState.ids.length > 1) {
      for (let i = 0; i < pathState.ids.length - 1; i++) {
        s.add(`${pathState.ids[i]}-${pathState.ids[i + 1]}`)
        s.add(`${pathState.ids[i + 1]}-${pathState.ids[i]}`)
      }
    }
    return s
  }, [pathState])

  useEffect(() => {
    api.graph().then((g) => {
      setAllNodes(g.nodes)
      setAllEdges(g.edges)
    })
  }, [])

  // Simple force-directed layout: 300 iterations of repulsion + spring.
  useEffect(() => {
    if (nodes.length === 0) return
    const pos: Record<number, { x: number; y: number; vx: number; vy: number }> = {}
    // Seed positions in a circle so the first render doesn't overlap.
    nodes.forEach((n, i) => {
      const angle = (i / nodes.length) * Math.PI * 2
      pos[n.id] = {
        x: width / 2 + Math.cos(angle) * 200,
        y: height / 2 + Math.sin(angle) * 200,
        vx: 0,
        vy: 0,
      }
    })
    const iterations = 300
    const repel = 4500
    const spring = 0.02
    const edgeLen = 140
    const damp = 0.82
    for (let it = 0; it < iterations; it++) {
      for (const a of nodes) {
        for (const b of nodes) {
          if (a.id === b.id) continue
          const dx = pos[a.id]!.x - pos[b.id]!.x
          const dy = pos[a.id]!.y - pos[b.id]!.y
          const d2 = dx * dx + dy * dy + 0.01
          const d = Math.sqrt(d2)
          const f = repel / d2
          pos[a.id]!.vx += (dx / d) * f
          pos[a.id]!.vy += (dy / d) * f
        }
      }
      for (const e of edges) {
        const a = pos[e.from_id]
        const b = pos[e.to_id]
        if (!a || !b) continue
        const dx = b.x - a.x
        const dy = b.y - a.y
        const d = Math.sqrt(dx * dx + dy * dy) + 0.01
        const f = (d - edgeLen) * spring
        a.vx += (dx / d) * f
        a.vy += (dy / d) * f
        b.vx -= (dx / d) * f
        b.vy -= (dy / d) * f
      }
      for (const n of nodes) {
        const p = pos[n.id]!
        p.vx *= damp
        p.vy *= damp
        p.x += p.vx
        p.y += p.vy
        p.x = Math.max(30, Math.min(width - 30, p.x))
        p.y = Math.max(30, Math.min(height - 30, p.y))
      }
    }
    const final: Record<number, { x: number; y: number }> = {}
    for (const n of nodes) final[n.id] = { x: pos[n.id]!.x, y: pos[n.id]!.y }
    setPositions(final)
  }, [nodes, edges])

  function onMouseDown(id: number) {
    return (e: React.MouseEvent) => {
      e.preventDefault()
      setDragging(id)
    }
  }
  function onMouseMove(e: React.MouseEvent) {
    if (dragging == null || !svgRef.current) return
    const rect = svgRef.current.getBoundingClientRect()
    const x = (e.clientX - rect.left) * (width / rect.width)
    const y = (e.clientY - rect.top) * (height / rect.height)
    setPositions((p) => ({ ...p, [dragging]: { x, y } }))
  }
  function onMouseUp() {
    setDragging(null)
  }

  const roleCounts = useMemo(() => {
    const c: Record<string, number> = {}
    for (const n of nodes) c[n.role] = (c[n.role] ?? 0) + 1
    return c
  }, [nodes])

  return (
    <div className="network page">
      <header className="page-head">
        <div className="page-head-title">
          <h1>🕸 {t('networkTitle')}</h1>
          <div className="muted small">
            {nodes.length}/{allNodes.length} {t('people')} · {edges.length}/{allEdges.length}{' '}
            {t('connections')} · {t('networkSub')}
          </div>
        </div>
        <div className="page-head-actions">
          <Input.Search
            placeholder={t('filterByName')}
            allowClear
            style={{ width: 200 }}
            value={nameFilter}
            onChange={(e) => setNameFilter(e.target.value)}
          />
          <Select
            mode="multiple"
            allowClear
            placeholder={t('filterRole')}
            style={{ minWidth: 180 }}
            value={roleFilter}
            onChange={setRoleFilter}
            maxTagCount="responsive"
            options={ROLE_ORDER.map((r) => ({
              value: r,
              label: `${roleMeta(r).icon} ${tk(t, 'role', r)}`,
            }))}
          />
          <Select
            mode="multiple"
            allowClear
            placeholder={t('filterRelKind')}
            style={{ minWidth: 200 }}
            value={kindFilter}
            onChange={setKindFilter}
            maxTagCount="responsive"
            options={REL_ORDER.map((k) => ({ value: k, label: tk(t, 'rel', k) }))}
          />
          <Button onClick={() => setPathModal(true)}>🧭 {t('findPath')}</Button>
          {focus && (
            <Button danger onClick={() => setFocus(null)}>
              {t('exitFocus')}
            </Button>
          )}
        </div>
      </header>

      <div className="page-body">
        {focus && (
          <Card size="small" style={{ marginBottom: 12 }}>
            <Space wrap>
              <span>
                🎯 {t('focusRoot')}: <b>{allNodes.find((n) => n.id === focus.id)?.name}</b>
              </span>
              <span>{t('expandHops')}:</span>
              <Segmented
                options={[1, 2, 3, 4].map((n) => ({ label: `${n} ${t('hop')}`, value: n }))}
                value={focus.hops}
                onChange={(v) => setFocus({ ...focus, hops: Number(v) })}
              />
              <span className="muted small">
                → {nodes.length} {t('inRadius')}
              </span>
            </Space>
          </Card>
        )}
        {pathState && pathState.ids.length > 1 && (
          <Card
            size="small"
            style={{ marginBottom: 12, borderLeft: '3px solid var(--accent)' }}
            extra={
              <Button size="small" onClick={() => setPathState(null)}>
                {t('clear')}
              </Button>
            }
          >
            🧭{' '}
            <b>
              {t('pathLabel')} ({pathState.hops} {t('hop')}):
            </b>{' '}
            {pathState.ids.map((id) => allNodes.find((n) => n.id === id)?.name ?? `#${id}`).join(' → ')}
          </Card>
        )}
        {pathState && pathState.ids.length === 0 && (
          <Card
            size="small"
            style={{ marginBottom: 12, borderLeft: '3px solid var(--warn)' }}
            extra={
              <Button size="small" onClick={() => setPathState(null)}>
                {t('close')}
              </Button>
            }
          >
            {t('noPath')}
          </Card>
        )}
        {aiPath && (
          <Card
            size="small"
            style={{ marginBottom: 12, borderLeft: '3px solid #ec4899' }}
            title={
              <span>
                🧠 {t('aiConnAnalysis')}: <b>{allNodes.find((n) => n.id === aiPath.from)?.name}</b> ↔{' '}
                <b>{allNodes.find((n) => n.id === aiPath.to)?.name}</b>
              </span>
            }
            extra={
              <Button size="small" onClick={() => setAiPath(null)}>
                {t('clear')}
              </Button>
            }
          >
            <div style={{ marginBottom: 8, fontStyle: 'italic', color: 'var(--muted)' }}>{aiPath.summary}</div>
            {aiPath.bfs_path_names && aiPath.bfs_path_names.length > 1 && (
              <div style={{ marginBottom: 8 }}>
                <Tag color="gold">{t('bfsPath')}</Tag> {aiPath.bfs_path_names.join(' → ')}
              </div>
            )}
            <Space direction="vertical" style={{ width: '100%' }}>
              {aiPath.connections.map((c, i) => {
                const colors: Record<string, string> = {
                  shared_interest: 'purple',
                  common_market: 'geekblue',
                  possible_bridge: 'orange',
                  explicit_path: 'gold',
                  weak_tie: 'default',
                  shared_person: 'magenta',
                }
                const icons: Record<string, string> = {
                  shared_interest: '🎯',
                  common_market: '📊',
                  possible_bridge: '🌉',
                  explicit_path: '🛤',
                  weak_tie: '➰',
                  shared_person: '👥',
                }
                const strengthColor =
                  c.strength === 'strong' ? 'green' : c.strength === 'medium' ? 'blue' : 'default'
                return (
                  <div key={i}>
                    <Tag color={colors[c.type] ?? 'default'}>
                      {icons[c.type] ?? '•'} {tk(t, 'conn', c.type)}
                    </Tag>
                    <Tag color={strengthColor}>{c.strength || 'unknown'}</Tag>
                    <span style={{ marginLeft: 4 }}>{c.detail}</span>
                  </div>
                )
              })}
            </Space>
          </Card>
        )}
        {common && (
          <Card
            size="small"
            style={{ marginBottom: 12, borderLeft: '3px solid #ec4899' }}
            title={
              <span>
                ✨ {t('commonWith')} <b>{allNodes.find((n) => n.id === common.focus_id)?.name}</b> —{' '}
                {common.themes.length} {t('themes')}, {common.highlight_ids.length} {t('customers')}
              </span>
            }
            extra={
              <Button size="small" onClick={() => setCommon(null)}>
                {t('clear')}
              </Button>
            }
          >
            <Space direction="vertical" style={{ width: '100%' }}>
              {common.themes.map((th, i) => (
                <div key={i}>
                  <Tag color="magenta">{th.theme}</Tag>
                  <span className="muted small"> {th.why}</span>
                  <div style={{ marginTop: 4 }}>
                    {th.customer_ids.map((cid) => {
                      const n = allNodes.find((x) => x.id === cid)
                      if (!n) return null
                      return (
                        <Button
                          key={cid}
                          size="small"
                          type="link"
                          style={{ padding: '0 6px' }}
                          onClick={() => setDrawerId(cid)}
                        >
                          {n.name}
                        </Button>
                      )
                    })}
                  </div>
                </div>
              ))}
            </Space>
          </Card>
        )}

        <div className="network-canvas card" style={{ position: 'relative' }}>
          {busy && (
            <div className="network-lock">
              <Spin size="large" tip={busy === 'common' ? t('aiBusyCommon') : t('aiBusyPath')}>
                <div style={{ padding: 60 }} />
              </Spin>
              <div className="muted small" style={{ textAlign: 'center', marginTop: 8 }}>
                {t('canSwitchTab')}
              </div>
            </div>
          )}
          <svg
            ref={svgRef}
            viewBox={`0 0 ${width} ${height}`}
            onMouseMove={onMouseMove}
            onMouseUp={onMouseUp}
            onMouseLeave={onMouseUp}
          >
            {edges.map((e) => {
              const a = positions[e.from_id]
              const b = positions[e.to_id]
              if (!a || !b) return null
              const active = hovered === e.from_id || hovered === e.to_id
              const onPath = pathEdgeSet.has(`${e.from_id}-${e.to_id}`)
              return (
                <g key={e.id}>
                  <line
                    x1={a.x}
                    y1={a.y}
                    x2={b.x}
                    y2={b.y}
                    stroke={onPath ? '#eab308' : active ? 'var(--accent)' : 'var(--muted)'}
                    strokeWidth={onPath ? 3 : active ? 2 : 1}
                    strokeDasharray={e.source === 'ai' ? '4 3' : undefined}
                    opacity={onPath ? 1 : hovered != null && !active ? 0.15 : 0.65}
                  />
                  {(active || onPath) && (
                    <text
                      x={(a.x + b.x) / 2}
                      y={(a.y + b.y) / 2}
                      fontSize={10}
                      fill={onPath ? '#eab308' : 'var(--accent)'}
                      textAnchor="middle"
                      style={{ pointerEvents: 'none' }}
                    >
                      {tk(t, 'rel', e.kind)}
                    </text>
                  )}
                </g>
              )
            })}
            {common &&
              common.focus_id &&
              positions[common.focus_id] &&
              common.highlight_ids.map((hid) => {
                const a = positions[common.focus_id]
                const b = positions[hid]
                if (!a || !b) return null
                return (
                  <line
                    key={'common-' + hid}
                    x1={a.x}
                    y1={a.y}
                    x2={b.x}
                    y2={b.y}
                    stroke="#ec4899"
                    strokeWidth={2}
                    strokeDasharray="6 4"
                    opacity={0.7}
                  />
                )
              })}
            {nodes.map((n) => {
              const p = positions[n.id]
              if (!p) return null
              const meta = roleMeta(n.role)
              const r = 20 + Math.min(12, n.interaction_count)
              const active = hovered === n.id
              const isFocus = common?.focus_id === n.id
              const isHighlight = common?.highlight_ids.includes(n.id) ?? false
              return (
                <g
                  key={n.id}
                  transform={`translate(${p.x},${p.y})`}
                  onMouseDown={onMouseDown(n.id)}
                  onMouseEnter={() => setHovered(n.id)}
                  onMouseLeave={() => setHovered(null)}
                  onClick={() => dragging == null && setDrawerId(n.id)}
                  style={{ cursor: 'pointer' }}
                >
                  {(isFocus || isHighlight) && (
                    <circle
                      r={r + 8}
                      fill="none"
                      stroke={isFocus ? '#ec4899' : '#f9a8d4'}
                      strokeWidth={isFocus ? 3 : 2}
                      opacity={0.85}
                    >
                      <animate attributeName="r" from={r + 4} to={r + 12} dur="1.4s" repeatCount="indefinite" />
                      <animate attributeName="opacity" from="0.85" to="0.15" dur="1.4s" repeatCount="indefinite" />
                    </circle>
                  )}
                  <circle
                    r={r}
                    fill={meta.color}
                    stroke={active ? 'white' : meta.color}
                    strokeWidth={active ? 3 : 1}
                    opacity={0.9}
                  />
                  <text y={4} fontSize={12} fill="white" textAnchor="middle" style={{ pointerEvents: 'none', fontWeight: 600 }}>
                    {meta.icon}
                  </text>
                  <text y={r + 14} fontSize={11} fill="var(--text)" textAnchor="middle" style={{ pointerEvents: 'none' }}>
                    {n.name}
                  </text>
                </g>
              )
            })}
          </svg>
        </div>

        <div className="net-legend card">
          <div className="section-title">{t('roles')}</div>
          <div className="net-legend-grid">
            {ROLE_ORDER.filter((r) => roleCounts[r]).map((r) => (
              <div key={r} className="net-legend-item">
                <span className="legend-dot" style={{ background: roleMeta(r).color }} />
                <span>{tk(t, 'role', r)}</span>
                <span className="muted">· {roleCounts[r]}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <NodeDrawer
        id={drawerId}
        allNodes={allNodes}
        t={t}
        onClose={() => setDrawerId(null)}
        onOpenCustomer={onPickCustomer}
        onSetFocus={(fid) => {
          setFocus({ id: fid, hops: 1 })
          setDrawerId(null)
        }}
        onFindPath={(from) => {
          setPathModal(true)
          setDrawerId(null)
          setTimeout(() => setPathState({ from, ids: [], hops: 0 }), 0)
        }}
        onFindCommon={async (cid) => {
          setDrawerId(null)
          setBusy('common')
          try {
            setCommon(await api.findCommon(cid))
            onBackgroundResult(`${t('foundCommonFor')} ${allNodes.find((n) => n.id === cid)?.name}.`)
          } catch (e) {
            alert(t('needLlm') + String(e))
          } finally {
            setBusy(null)
          }
        }}
      />

      <PathFinderModal
        open={pathModal}
        allNodes={allNodes}
        t={t}
        initialFrom={pathState?.from}
        onClose={() => setPathModal(false)}
        onAiSearch={async (from, to) => {
          setPathModal(false)
          setBusy('ai_path')
          try {
            const r = await api.pathAi(from, to)
            setAiPath({
              from: r.from,
              to: r.to,
              summary: r.summary,
              connections: r.connections,
              bfs_path_names: r.bfs_path_names,
            })
            onBackgroundResult(
              `${t('analyzedConn')} ${allNodes.find((n) => n.id === from)?.name} ↔ ${
                allNodes.find((n) => n.id === to)?.name
              }.`,
            )
          } catch (e) {
            alert(t('needLlm') + String(e))
          } finally {
            setBusy(null)
          }
        }}
      />
    </div>
  )
}

function NodeDrawer({
  id,
  allNodes,
  t,
  onClose,
  onOpenCustomer,
  onSetFocus,
  onFindPath,
  onFindCommon,
}: {
  id: number | null
  allNodes: GraphNode[]
  t: T
  onClose: () => void
  onOpenCustomer: (id: number) => void
  onSetFocus: (id: number) => void
  onFindPath: (fromId: number) => void
  onFindCommon: (id: number) => Promise<void>
}) {
  const [commonBusy, setCommonBusy] = useState(false)
  const node = id != null ? allNodes.find((n) => n.id === id) : null
  const [similar, setSimilar] = useState<Array<{ customer: Customer; score: number; reasons: string[] }>>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (id == null) return
    setLoading(true)
    api
      .similar(id)
      .then((r) => {
        setSimilar(r.similar)
        setLoading(false)
      })
      .catch(() => setLoading(false))
  }, [id])

  if (!node) return <Drawer open={false} onClose={onClose} />
  const meta = roleMeta(node.role)
  return (
    <Drawer
      open={id != null}
      onClose={onClose}
      width={380}
      title={
        <Space>
          <Avatar name={node.name} url={node.avatar_url} size={36} />
          <div>
            <div style={{ fontWeight: 600 }}>{node.name}</div>
            <div style={{ color: 'var(--muted)', fontSize: 12 }}>{node.company || t('unknownCompany')}</div>
          </div>
        </Space>
      }
    >
      <div style={{ marginBottom: 12 }}>
        <Tag color={meta.color}>
          {meta.icon} {tk(t, 'role', node.role)}
        </Tag>
        <span className="muted small" style={{ marginLeft: 8 }}>
          {node.interaction_count} {t('interactionsCount')}
        </span>
      </div>

      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Space wrap>
          <Button type="primary" onClick={() => onSetFocus(node.id)}>
            🎯 {t('setAsRoot')}
          </Button>
          <Button onClick={() => onOpenCustomer(node.id)}>{t('openProfile')}</Button>
          <Button onClick={() => onFindPath(node.id)}>🧭 {t('findPathFrom')}</Button>
          <Button
            loading={commonBusy}
            style={{ background: '#ec4899', color: 'white', borderColor: '#ec4899' }}
            onClick={async () => {
              setCommonBusy(true)
              try {
                await onFindCommon(node.id)
              } finally {
                setCommonBusy(false)
              }
            }}
          >
            ✨ {t('aiFindCommon')}
          </Button>
        </Space>

        <Card size="small" title={<span>✨ {t('similarCustomers')}</span>}>
          {loading && <div className="muted">{t('computing')}</div>}
          {!loading && similar.length === 0 && <div className="empty small">{t('noSimilar')}</div>}
          {similar.map((s) => (
            <div key={s.customer.id} className="sim-row">
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <Avatar name={s.customer.name} url={s.customer.avatar_url} size={28} />
                <Button
                  type="link"
                  style={{ padding: 0, fontWeight: 500 }}
                  onClick={() => onOpenCustomer(s.customer.id)}
                >
                  {s.customer.name}
                </Button>
                <Tag>{s.score.toFixed(2)}</Tag>
              </div>
              <ul style={{ margin: '4px 0 6px 22px', padding: 0, color: 'var(--muted)', fontSize: 12 }}>
                {s.reasons.map((r, i) => (
                  <li key={i}>{r}</li>
                ))}
              </ul>
            </div>
          ))}
        </Card>
      </Space>
    </Drawer>
  )
}

function PathFinderModal({
  open,
  allNodes,
  t,
  initialFrom,
  onClose,
  onAiSearch,
}: {
  open: boolean
  allNodes: GraphNode[]
  t: T
  initialFrom?: number
  onClose: () => void
  onAiSearch: (from: number, to: number) => Promise<void>
}) {
  const [from, setFrom] = useState<number | undefined>(initialFrom)
  const [to, setTo] = useState<number | undefined>()

  useEffect(() => {
    if (open) setFrom(initialFrom)
  }, [open, initialFrom])

  const opts = allNodes.map((n) => ({ value: n.id, label: `${n.name} (${tk(t, 'role', n.role)})` }))
  const disabled = from == null || to == null || from === to

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title={`🧠 ${t('pathModalTitle')}`}
      width={560}
      footer={[
        <Button key="c" onClick={onClose}>
          {t('cancel')}
        </Button>,
        <Button
          key="ai"
          type="primary"
          disabled={disabled}
          onClick={() => from != null && to != null && onAiSearch(from, to)}
          style={{ background: '#ec4899', borderColor: '#ec4899' }}
        >
          🧠 {t('aiSearchBtn')}
        </Button>,
      ]}
    >
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <div>
          <div style={{ marginBottom: 4, color: 'var(--muted)' }}>{t('from')}:</div>
          <Select
            showSearch
            style={{ width: '100%' }}
            value={from}
            onChange={setFrom}
            options={opts}
            optionFilterProp="label"
            placeholder={t('pickCustomerDash')}
          />
        </div>
        <div>
          <div style={{ marginBottom: 4, color: 'var(--muted)' }}>{t('to')}:</div>
          <Select
            showSearch
            style={{ width: '100%' }}
            value={to}
            onChange={setTo}
            options={opts.filter((o) => o.value !== from)}
            optionFilterProp="label"
            placeholder={t('pickCustomerDash')}
          />
        </div>
        <div className="muted small">{t('pathModalHint')}</div>
      </Space>
    </Modal>
  )
}
