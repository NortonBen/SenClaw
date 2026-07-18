import { Card, Statistic } from 'antd'
import { STAGE_COLORS, STAGE_ORDER } from '../constants'
import { tk, type T } from '../i18n'

export function StatTile({
  label,
  value,
  sub,
  accent,
  warn,
  onClick,
}: {
  label: string
  value: string
  sub?: string
  accent: string
  warn?: boolean
  onClick?: () => void
}) {
  return (
    <Card
      hoverable={!!onClick}
      onClick={onClick}
      className={'stattile-card' + (warn ? ' warn' : '')}
      styles={{ body: { padding: 16, borderLeft: `4px solid ${accent}` } }}
    >
      <Statistic
        title={label}
        value={value}
        valueStyle={{ color: warn ? 'var(--warn)' : accent, fontSize: 26, fontWeight: 700 }}
      />
      {sub && <div style={{ color: 'var(--muted)', fontSize: 12, marginTop: 3 }}>{sub}</div>}
    </Card>
  )
}

/// Compact money format for tiles: 1.2B, 1.2M, 400k, 12.
export function formatShortMoney(n: number): string {
  if (!n) return '0'
  if (Math.abs(n) >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`
  if (Math.abs(n) >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (Math.abs(n) >= 1_000) return `${Math.round(n / 1_000)}k`
  return String(Math.round(n))
}

/// Proportional stacked bar over the deal stages + a legend underneath.
export function StageBar({
  byStage,
  t,
}: {
  byStage: Record<string, { count: number; value: number }>
  t: T
}) {
  const entries = STAGE_ORDER.map((s) => ({ stage: s, ...(byStage[s] ?? { count: 0, value: 0 }) }))
  const total = entries.reduce((sum, e) => sum + e.value, 0)
  return (
    <div>
      <div className="stagebar">
        {entries.map((e) => {
          const pct = total > 0 ? (e.value / total) * 100 : 0
          if (pct === 0) return null
          return (
            <div
              key={e.stage}
              className="stagebar-seg"
              style={{ width: pct + '%', background: STAGE_COLORS[e.stage] }}
              title={`${tk(t, 'dealStage', e.stage)}: ${e.count} · ${formatShortMoney(e.value)}`}
            >
              {pct >= 12 ? tk(t, 'dealStage', e.stage) : ''}
            </div>
          )
        })}
        {total === 0 && <div className="stagebar-empty">{t('noDealsToShow')}</div>}
      </div>
      <div className="stage-legend">
        {entries.map((e) => (
          <div key={e.stage} className="legend-item">
            <span className="legend-dot" style={{ background: STAGE_COLORS[e.stage] }} />
            <span className="legend-label">{tk(t, 'dealStage', e.stage)}</span>
            <span className="legend-count">
              {e.count} · {formatShortMoney(e.value)}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

/// Horizontal bar list — the dashboard's "value by X" cards, and the
/// `horizontalBarChart` display type. Deliberately not recharts: one bar per
/// row reads better than a chart at this size, and it keeps the bundle free of
/// a charting dep.
///
/// `showValues` is the `…WithLabels` half of the display-type pair, and
/// `mirror` is that chart's `reverseX` — the value axis running right-to-left.
/// Row order is the caller's business (`reverseY` reverses the array).
export function BarList({
  rows,
  format = (n: number) => String(n),
  color = '#5e4ae3',
  empty,
  showValues = true,
  mirror = false,
}: {
  rows: Array<{ label: string; value: number; color?: string }>
  format?: (n: number) => string
  color?: string
  empty: string
  showValues?: boolean
  mirror?: boolean
}) {
  const max = Math.max(1, ...rows.map((r) => r.value))
  if (rows.length === 0) return <div className="empty small">{empty}</div>
  return (
    <div className="barlist">
      {rows.map((r, i) => (
        // Buckets are distinct by construction (GROUP BY), but a chart grouped
        // by a relation can still surface two blank names — so the index keeps
        // the key unique.
        <div key={`${r.label}-${i}`} className="barlist-row">
          <div className="barlist-label" title={r.label}>
            {r.label}
          </div>
          <div className="barlist-track" style={{ justifyContent: mirror ? 'flex-end' : 'flex-start' }}>
            <div
              className="barlist-fill"
              style={{ width: `${(r.value / max) * 100}%`, background: r.color ?? color }}
            />
          </div>
          {showValues && <div className="barlist-value">{format(r.value)}</div>}
        </div>
      ))}
    </div>
  )
}
