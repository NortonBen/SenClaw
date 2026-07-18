// One card of the dynamic dashboard grid: a chart spec + its resolved data.
//
// Every display type is hand-drawn (flex bars, or SVG for the round ones), the
// same call the app already made for BarList and the network graph — a chart
// library would be a big dependency for six shapes this simple.

import { Button, Tooltip } from 'antd'
import { CopyOutlined, DeleteOutlined, EditOutlined, HolderOutlined } from '@ant-design/icons'
import {
  fmtDate,
  formatMoney,
  type ChartCell,
  type ChartDisplay,
  type ChartFilter,
  type ChartResult,
  type DashField,
  type DashSchema,
} from '../api'
import { CHART_FIELD_VOCAB, CHART_PALETTE, DEFAULT_CURRENCY } from '../constants'
import { fmt, tk, type T } from '../i18n'
import { BarList, formatShortMoney } from './StatTile'

// ---- labels, colours, formatting ----

/// A raw bucket ("direct_customer", "won", "1") as a human reads it. Enum
/// fields get their app-wide label; anything open-set (an org name, an
/// industry) is already human and passes through.
export function chartBucketLabel(t: T, element: string, field: string, bucket: string): string {
  if (bucket === '') return t('blankBucket')
  const vocab = CHART_FIELD_VOCAB[`${element}.${field}`]
  return vocab ? tk(t, vocab.prefix, bucket) : bucket
}

/// The colour a bucket gets with no override: its own semantic colour where the
/// app has one (won is green everywhere), else the rotating palette.
export function defaultBucketColor(element: string, grouping: string, bucket: string, i: number): string {
  const vocab = CHART_FIELD_VOCAB[`${element}.${grouping}`]
  return vocab?.colors?.[bucket] ?? CHART_PALETTE[i % CHART_PALETTE.length]!
}

/// Colour per bucket, most specific first: an explicit override from the
/// display blob, then the default above.
function bucketColor(element: string, grouping: string, bucket: string, i: number, display: ChartDisplay): string {
  return display.colors?.[i] ?? defaultBucketColor(element, grouping, bucket, i)
}

/// Compact, for labels inside a chart — a full VND amount would not fit.
function makeFormat(isMoney: boolean) {
  return (v: number) => (isMoney ? formatShortMoney(v) : Number.isInteger(v) ? v.toLocaleString() : v.toFixed(2))
}

/// Whether a money total may be shown as an amount at all.
///
/// The server reports which currencies it summed. Exactly one → that currency.
/// More than one → `SUM` added EUR to VND and the number is not an amount in
/// any currency, so stamping a symbol on it would be a lie; print the bare
/// figure and let the caller flag it. None reported (older data, nothing
/// matched) → fall back to the house currency the KPI row already assumes.
function moneyCurrency(data: ChartResult): string | null {
  if (!data.is_money) return null
  if (data.currencies.length > 1) return null
  return data.currencies[0] ?? DEFAULT_CURRENCY
}

/// Full, for the one headline number per card.
function headline(v: number, data: ChartResult): string {
  if (!data.is_money) return v.toLocaleString()
  const cur = moneyCurrency(data)
  return cur ? formatMoney(v, cur) : v.toLocaleString()
}

function findField(schema: DashSchema | null, element: string, key: string): DashField | undefined {
  return schema?.elements.find((e) => e.key === element)?.fields.find((f) => f.key === key)
}

/// The filter phrases under the title, in the user's language.
///
/// Deliberately rebuilt from `chart.filters` rather than parsed out of the
/// server's `filter_summary`: that string is built from raw column keys and
/// English operators, and picking it apart to translate it would couple the UI
/// to its exact wording. The server's version is still the fallback below,
/// for the moment before the schema has loaded and value kinds are unknown.
export function localizeFilters(
  t: T,
  schema: DashSchema | null,
  element: string,
  filters: ChartFilter[],
): string[] {
  const valueText = (field: string, v: string | number | boolean) => {
    const kind = findField(schema, element, field)?.kind
    // Dates are stored as epoch seconds; printing the raw integer would be
    // useless to a human reading "created after 1750000000".
    if (kind === 'date') return fmtDate(Number(v))
    // "amount > 50,000,000" is scannable; "amount > 50000000" is a digit-count
    // exercise.
    if (kind === 'number' && typeof v === 'number') return v.toLocaleString()
    return chartBucketLabel(t, element, field, String(v))
  }
  return filters.map((f) => {
    const label = tk(t, 'fld', f.field)
    const vals = f.values.map((v) => valueText(f.field, v))
    switch (f.op) {
      case 'isNull':
      case 'isNotNull':
        return `${label} ${tk(t, 'op', f.op)}`
      case 'inLastDays':
        // The operand is a DAY COUNT, not a timestamp — even though the field
        // itself is a date. Running it through valueText would format 7 as
        // 01/01/1970 (seven seconds past the epoch).
        return fmt(t('filterPhrase_inLastDays'), { a: label, b: String(f.values[0] ?? '') })
      case 'between':
        return fmt(t('filterPhrase_between'), { a: label, b: vals[0] ?? '', c: vals[1] ?? '' })
      default:
        return `${label} ${tk(t, 'op', f.op)} ${vals.join(', ')}`
    }
  })
}

// ---- the shapes ----

type Series = { label: string; value: number; color: string }

/// Columns. `flip` is the chart's `reverseY`: the value axis pointing down, so
/// bars hang from the top instead of standing on the baseline.
function VerticalBars({
  series,
  showValues,
  flip,
  format,
}: {
  series: Series[]
  showValues: boolean
  flip: boolean
  format: (n: number) => string
}) {
  const max = Math.max(1, ...series.map((s) => s.value))
  return (
    <div className={'vbars' + (flip ? ' flip' : '')}>
      {series.map((s, i) => (
        <div className="vbar-col" key={`${s.label}-${i}`} title={`${s.label}: ${format(s.value)}`}>
          {showValues && !flip && <div className="vbar-value">{format(s.value)}</div>}
          <div className="vbar-track">
            <div className="vbar-fill" style={{ height: `${(s.value / max) * 100}%`, background: s.color }} />
          </div>
          {showValues && flip && <div className="vbar-value">{format(s.value)}</div>}
          <div className="vbar-label" title={s.label}>
            {s.label}
          </div>
        </div>
      ))}
    </div>
  )
}

/// SVG donut + legend. Each slice is one arc of a single circle, drawn with a
/// dash pattern — no path maths, and no chart library.
function Doughnut({
  series,
  total,
  format,
  t,
}: {
  series: Series[]
  total: number
  format: (n: number) => string
  t: T
}) {
  const R = 54
  const C = 2 * Math.PI * R
  let acc = 0
  return (
    <div className="donut-wrap">
      <svg viewBox="0 0 140 140" className="donut" role="img">
        {/* The track shows through when every bucket is zero, so the chart
            still reads as "a donut with nothing in it" rather than blank. */}
        <circle cx={70} cy={70} r={R} fill="none" strokeWidth={22} className="donut-track" />
        <g transform="rotate(-90 70 70)">
          {series.map((s, i) => {
            const len = total > 0 ? (s.value / total) * C : 0
            const el = (
              <circle
                key={`${s.label}-${i}`}
                cx={70}
                cy={70}
                r={R}
                fill="none"
                stroke={s.color}
                strokeWidth={22}
                strokeDasharray={`${len} ${C - len}`}
                strokeDashoffset={-acc}
              >
                <title>{`${s.label}: ${format(s.value)}`}</title>
              </circle>
            )
            acc += len
            return el
          })}
        </g>
        <text x={70} y={68} textAnchor="middle" className="donut-total">
          {format(total)}
        </text>
        <text x={70} y={84} textAnchor="middle" className="donut-sub">
          {series.length} {t('groupsLabel')}
        </text>
      </svg>
      <div className="donut-legend">
        {series.map((s, i) => (
          <div className="legend-item" key={`${s.label}-${i}`}>
            <span className="legend-dot" style={{ background: s.color }} />
            <span className="legend-label" title={s.label}>
              {s.label}
            </span>
            <span className="legend-count">{format(s.value)}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

/// SVG radar. Values are scaled against the largest bucket, so the shape shows
/// relative weight — an absolute scale would collapse to a dot whenever one
/// bucket dominates.
function RadarChart({ series, format }: { series: Series[]; format: (n: number) => string }) {
  const SIZE = 190
  /// Side gutters for the axis labels. The web is round but the text is wide:
  /// an end-anchored label on the left axis runs outwards from the plot, so
  /// without this the viewBox clips it ("Nhà cung cấp" → "g cấp").
  const PAD = 46
  const c = SIZE / 2
  const R = c - 30
  const n = series.length
  const max = Math.max(1, ...series.map((s) => s.value))
  const pt = (i: number, frac: number): [number, number] => {
    const a = (Math.PI * 2 * i) / n - Math.PI / 2
    return [c + Math.cos(a) * R * frac, c + Math.sin(a) * R * frac]
  }
  const ring = (frac: number) => series.map((_, i) => pt(i, frac).join(',')).join(' ')
  const stroke = series[0]?.color ?? CHART_PALETTE[0]!

  return (
    <div className="radar-wrap">
      <svg viewBox={`${-PAD} 0 ${SIZE + PAD * 2} ${SIZE}`} className="radar" role="img">
        {[0.25, 0.5, 0.75, 1].map((f) => (
          <polygon key={f} points={ring(f)} className="radar-grid" />
        ))}
        {series.map((_, i) => {
          const [x, y] = pt(i, 1)
          return <line key={i} x1={c} y1={c} x2={x} y2={y} className="radar-axis" />
        })}
        <polygon
          points={series.map((s, i) => pt(i, s.value / max).join(',')).join(' ')}
          className="radar-poly"
          style={{ fill: stroke + '40', stroke }}
        />
        {series.map((s, i) => {
          const [x, y] = pt(i, s.value / max)
          return (
            <circle key={`${s.label}-${i}`} cx={x} cy={y} r={3.5} fill={s.color}>
              <title>{`${s.label}: ${format(s.value)}`}</title>
            </circle>
          )
        })}
        {series.map((s, i) => {
          const [x, y] = pt(i, 1.17)
          const anchor = x < c - 2 ? 'end' : x > c + 2 ? 'start' : 'middle'
          return (
            <text key={`${s.label}-${i}`} x={x} y={y + 3} textAnchor={anchor} className="radar-label">
              <title>{`${s.label}: ${format(s.value)}`}</title>
              {s.label.length > 14 ? s.label.slice(0, 13) + '…' : s.label}
            </text>
          )
        })}
      </svg>
    </div>
  )
}

// ---- body ----

/// The chart itself, without the card chrome — shared with the builder's live
/// preview so what you preview is literally what you get.
export function ChartBody({
  element,
  grouping,
  display,
  data,
  t,
}: {
  element: string
  grouping: string
  display: ChartDisplay
  data: ChartResult
  t: T
}) {
  const format = makeFormat(data.is_money)
  if (data.rows.length === 0) return <div className="empty small">{t('empty')}</div>

  // No grouping is a single total, and a one-bar bar chart is a worse way to
  // read one number than the number.
  if (!grouping) {
    return <div className="chart-bignum">{headline(data.rows[0]!.value, data)}</div>
  }

  const type = display.type ?? 'verticalBarChart'
  let series: Series[] = data.rows.map((r, i) => ({
    label: chartBucketLabel(t, element, grouping, r.bucket),
    value: r.value,
    color: bucketColor(element, grouping, r.bucket, i, display),
  }))

  switch (type) {
    case 'doughnutChart':
      // The category axis of a donut is the order slices are laid down.
      if (display.reverseX) series = [...series].reverse()
      return <Doughnut series={series} total={data.total} format={format} t={t} />

    case 'radarChart':
      if (display.reverseX) series = [...series].reverse()
      // Two axes make a line and one makes a dot; below three, bars are the
      // honest rendering of the same numbers.
      if (series.length < 3) return <VerticalBars series={series} showValues flip={false} format={format} />
      return <RadarChart series={series} format={format} />

    case 'horizontalBarChart':
    case 'horizontalBarChartWithLabels':
      // Categories run down the Y axis here, so reverseY reorders the rows and
      // reverseX mirrors the value axis.
      if (display.reverseY) series = [...series].reverse()
      return (
        <BarList
          rows={series}
          format={format}
          empty={t('empty')}
          showValues={type === 'horizontalBarChartWithLabels'}
          mirror={!!display.reverseX}
        />
      )

    case 'verticalBarChart':
    case 'verticalBarChartWithLabels':
    default:
      // Categories run along the X axis, so the roles swap over.
      if (display.reverseX) series = [...series].reverse()
      return (
        <VerticalBars
          series={series}
          showValues={type === 'verticalBarChartWithLabels'}
          flip={!!display.reverseY}
          format={format}
        />
      )
  }
}

// ---- card ----

export function ChartCard({
  cell,
  schema,
  t,
  onEdit,
  onDuplicate,
  onDelete,
}: {
  cell: ChartCell
  schema: DashSchema | null
  t: T
  onEdit: () => void
  onDuplicate: () => void
  onDelete: () => void
}) {
  const { chart, data, error } = cell
  const display = chart.display ?? {}

  // Grouped: "€1,809,500 · 3 groups", the reference's subtitle. Ungrouped: the
  // big number below already IS the total, so name the metric instead of
  // printing it twice.
  let subtitle: React.ReactNode = fmt(tk(t, 'metricLabel', chart.metric), {
    a: tk(t, 'el', chart.element),
  })
  if (data && chart.grouping) {
    subtitle = (
      <>
        {headline(data.total, data)}
        {/* SUM across currencies is not an amount in any of them. Say so next
            to the number rather than let a bare figure read as a total. */}
        {data.is_money && data.currencies.length > 1 && (
          <>
            {' '}
            <Tooltip title={fmt(t('mixedCurrencyHint'), { a: data.currencies.join(', ') })}>
              <span className="chart-trunc">{t('mixedCurrency')}</span>
            </Tooltip>
          </>
        )}{' '}
        ·{' '}
        {data.truncated ? (
          // `total` sums only the buckets that came back, so a truncated chart
          // must never present itself as the whole picture.
          <Tooltip title={fmt(t('truncatedHint'), { a: data.groups })}>
            <span className="chart-trunc">{fmt(t('truncatedGroups'), { a: data.groups })}</span>
          </Tooltip>
        ) : (
          `${data.groups} ${t('groupsLabel')}`
        )}
      </>
    )
  }

  const filters = display.showFilters && chart.filters.length > 0
      ? (schema ? localizeFilters(t, schema, chart.element, chart.filters) : data?.filter_summary ?? [])
      : []

  return (
    <div className={'card chart-card' + (error ? ' has-error' : '')}>
      <div className="chart-head">
        <div className="chart-head-main">
          <div className="chart-name">
            <HolderOutlined className="chart-drag" />
            <span className="chart-name-text" title={chart.name}>
              {chart.name}
            </span>
            {chart.is_template && <span className="chart-badge">{t('chartTemplateBadge')}</span>}
          </div>
          <div className="chart-sub">{subtitle}</div>
          {filters.length > 0 && <div className="chart-filter-sum">{filters.join(' · ')}</div>}
        </div>
        <div className="chart-actions">
          <Tooltip title={t('edit')}>
            <Button size="small" type="text" icon={<EditOutlined />} onClick={onEdit} />
          </Tooltip>
          <Tooltip title={t('duplicate')}>
            <Button size="small" type="text" icon={<CopyOutlined />} onClick={onDuplicate} />
          </Tooltip>
          <Tooltip title={t('del')}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={onDelete} />
          </Tooltip>
        </div>
      </div>

      <div className="chart-body">
        {error ? (
          // One chart that no longer compiles is a broken card, not a broken
          // dashboard — the server hands us the reason, so show it.
          <div className="chart-err">
            <div className="chart-err-title">⚠ {t('chartError')}</div>
            <div className="chart-err-msg">{error}</div>
          </div>
        ) : data ? (
          <ChartBody element={chart.element} grouping={chart.grouping} display={display} data={data} t={t} />
        ) : null}
      </div>
    </div>
  )
}
