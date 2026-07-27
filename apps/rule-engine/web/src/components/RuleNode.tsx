// The one and only node renderer. Handles are generated from the rule spec
// plus the node's config, never hard-coded per rule type.

import { memo, useMemo } from 'react'
import { Handle, Position, type Node, type NodeProps } from '@xyflow/react'
import { Tooltip, theme } from 'antd'
import { portsOf } from '../ports'
import type { JsonObject, NodeOpts, PortSpec, RuleSpec } from '../types'

export const NODE_W = 230
const HEADER_H = 34
const NAME_H = 24
/** Vertical distance between two handles. The original used 10px and the
 *  switch ports collided; 24 keeps the labels readable. */
export const PORT_GAP = 24
const BOTTOM_PAD = 10

export interface RuleNodeData extends Record<string, unknown> {
  ruleId: string
  name: string
  config: JsonObject
  opts: NodeOpts
  debug: boolean
  spec?: RuleSpec
  errors: string[]
  warnings: string[]
  flash: boolean
}

export type RuleFlowNode = Node<RuleNodeData, 'rule'>

export function nodeHeight(rows: number): number {
  return HEADER_H + NAME_H + Math.max(rows, 1) * PORT_GAP + BOTTOM_PAD
}

/** Relative to `.rule-node__ports`, which starts below the header + name row. */
function handleTop(index: number): number {
  return index * PORT_GAP + PORT_GAP / 2
}

function PortRow({
  port,
  index,
  side,
}: {
  port: PortSpec
  index: number
  side: 'in' | 'out'
}) {
  const top = handleTop(index)
  const color = port.color || (side === 'in' ? '#8c8c8c' : '#52c41a')
  const title = port.description ? `${port.label} — ${port.description}` : port.label
  return (
    <>
      <Tooltip title={title} placement={side === 'in' ? 'left' : 'right'}>
        <Handle
          type={side === 'in' ? 'target' : 'source'}
          position={side === 'in' ? Position.Left : Position.Right}
          id={port.id}
          style={{ top, background: color }}
        />
      </Tooltip>
      <div
        className={`rule-node__port-label ${side === 'in' ? 'is-in' : 'is-out'}`}
        style={{ top: top - 5, color }}
      >
        {port.label}
      </div>
    </>
  )
}

function RuleNodeInner({ data, selected }: NodeProps<RuleFlowNode>) {
  const { token } = theme.useToken()
  const spec = data.spec
  const { inputs, outputs } = useMemo(
    () => portsOf(spec, data.config ?? {}),
    [spec, data.config],
  )
  const rows = Math.max(inputs.length, outputs.length)
  const color = spec?.color ?? '#8c8c8c'
  const hasError = data.errors.length > 0
  const hasWarning = data.warnings.length > 0

  const classes = [
    'rule-node',
    selected ? 'is-selected' : '',
    hasError ? 'is-error' : '',
    data.flash ? 'is-flash' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <div
      className={classes}
      style={{
        width: NODE_W,
        minHeight: nodeHeight(rows),
        background: token.colorBgContainer,
        border: `1px solid ${hasError ? token.colorError : color}`,
        color: token.colorText,
      }}
    >
      <div
        className="rule-node__header"
        style={{ background: `${color}1A`, borderBottom: `1px solid ${color}55` }}
      >
        <span style={{ fontSize: 15 }}>{spec?.icon ?? '❓'}</span>
        <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>
          {spec?.name ?? data.ruleId}
        </span>
        {data.debug && (
          <Tooltip title="Node bật debug — ghi trace từng bước">
            <span>🐞</span>
          </Tooltip>
        )}
        {hasError && (
          <Tooltip title={data.errors.join('\n')}>
            <span style={{ color: token.colorError }}>⛔</span>
          </Tooltip>
        )}
        {!hasError && hasWarning && (
          <Tooltip title={data.warnings.join('\n')}>
            <span style={{ color: token.colorWarning }}>⚠️</span>
          </Tooltip>
        )}
      </div>

      <Tooltip title={data.name}>
        <div className="rule-node__name" style={{ color: token.colorTextSecondary }}>
          {data.name || '(chưa đặt tên)'}
        </div>
      </Tooltip>

      <div className="rule-node__ports" style={{ height: rows * PORT_GAP }}>
        {inputs.map((p, i) => (
          <PortRow key={`in-${p.id}`} port={p} index={i} side="in" />
        ))}
        {outputs.map((p, i) => (
          <PortRow key={`out-${p.id}`} port={p} index={i} side="out" />
        ))}
      </div>
    </div>
  )
}

export default memo(RuleNodeInner)
