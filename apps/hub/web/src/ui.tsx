// Small shared UI helpers used across views.
import { Space, Tag, Typography } from 'antd'

const { Text } = Typography

export function formatVal(v: unknown): string {
  if (v === null || v === undefined) return '—'
  if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(2)
  if (typeof v === 'object') return JSON.stringify(v)
  return String(v)
}

export function levelColor(level: string): string {
  switch (level.toLowerCase()) {
    case 'critical':
    case 'error':
    case 'failed':
      return 'red'
    case 'warning':
      return 'gold'
    case 'success':
      return 'green'
    default:
      return 'blue'
  }
}

export function AttrTags({
  attributes,
  max,
}: {
  attributes: Record<string, unknown>
  max: number
}) {
  const entries = Object.entries(attributes ?? {}).slice(0, max)
  if (entries.length === 0) return null
  return (
    <div style={{ marginTop: 10 }}>
      <Space size={[6, 6]} wrap>
        {entries.map(([k, v]) => (
          <Tag key={k} bordered>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {k}
            </Text>{' '}
            {formatVal(v)}
          </Tag>
        ))}
      </Space>
    </div>
  )
}

export function Sparkline({ values }: { values: number[] }) {
  const w = 320
  const h = 64
  const min = Math.min(...values)
  const max = Math.max(...values)
  const span = max - min || 1
  const pts = values
    .map((v, i) => `${(i / (values.length - 1)) * w},${h - 4 - ((v - min) / span) * (h - 8)}`)
    .join(' ')
  return (
    <svg className="spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none">
      <polygon className="area" points={`0,${h} ${pts} ${w},${h}`} />
      <polyline points={pts} />
    </svg>
  )
}

export const POLL_MS = 5000
