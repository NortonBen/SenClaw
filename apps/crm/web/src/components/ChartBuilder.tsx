// The chart builder modal — the reference CRM's "+ Add Chart" panel.
//
// Every dropdown is rendered from `/dashboard/schema`, so which metrics and
// groupings an element supports lives in exactly one place (the Rust registry)
// and this file cannot drift from it.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  Button,
  Checkbox,
  Collapse,
  ColorPicker,
  DatePicker,
  Input,
  InputNumber,
  Modal,
  Select,
  Switch,
  Tooltip,
} from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import dayjs from 'dayjs'
import {
  api,
  type Chart,
  type ChartFilter,
  type ChartInput,
  type ChartResult,
  type DashElement,
  type DashField,
  type DashSchema,
} from '../api'
import { fmt, tk, type T } from '../i18n'
import { ChartBody, chartBucketLabel, defaultBucketColor } from './ChartCard'

/// How many series get a colour swatch. Past a handful the row stops being a
/// control and starts being a wall.
const MAX_COLOR_SWATCHES = 8

type Draft = Omit<ChartInput, 'display'> & { display: NonNullable<Chart['display']> }

function emptyDraft(schema: DashSchema): Draft {
  const el = schema.elements[0]
  return {
    name: '',
    element: el?.key ?? 'contact',
    metric: el?.metrics[0]?.key ?? 'count',
    grouping: '',
    filters: [],
    display: { type: schema.displayTypes[0] ?? 'verticalBarChart', showFilters: true },
    size: 'medium',
    is_template: false,
  }
}

function draftOf(c: Chart): Draft {
  return {
    name: c.name,
    element: c.element,
    metric: c.metric,
    grouping: c.grouping,
    filters: c.filters,
    display: c.display ?? {},
    size: c.size,
    is_template: c.is_template,
  }
}

/// The server's message, not "Error: the server's message".
function msg(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function ChartBuilder({
  chart,
  schema,
  t,
  onClose,
  onSaved,
}: {
  /// null = create a new chart.
  chart: Chart | null
  schema: DashSchema
  t: T
  onClose: () => void
  onSaved: () => void
}) {
  const initial = useMemo(() => (chart ? draftOf(chart) : emptyDraft(schema)), [chart, schema])
  const [draft, setDraft] = useState<Draft>(initial)
  const [preview, setPreview] = useState<ChartResult | null>(null)
  const [previewErr, setPreviewErr] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const element: DashElement | undefined = schema.elements.find((e) => e.key === draft.element)

  // ---- filter value vocabularies ----
  // Open sets (industry, source, org names) come from the data. The ref guards
  // against a re-fetch storm: the effect below runs on every keystroke in the
  // modal, and a Set of in-flight keys is cheaper than threading a cache
  // through the dependency array.
  const [values, setValues] = useState<Record<string, string[]>>({})
  const asked = useRef<Set<string>>(new Set())
  const loadValues = useCallback(async (el: string, field: string) => {
    const k = `${el}.${field}`
    if (asked.current.has(k)) return
    asked.current.add(k)
    try {
      const got = await api.chartFieldValues(el, field)
      setValues((v) => ({ ...v, [k]: got }))
    } catch {
      // Leave it unset — the picker still accepts free text.
    }
  }, [])

  // ---- live preview ----
  // Debounced, because this runs a real query and the name field alone would
  // otherwise fire one per keystroke. `name`/`display`/`size` are deliberately
  // NOT dependencies: they change nothing about the numbers.
  useEffect(() => {
    const h = setTimeout(async () => {
      try {
        setPreview(
          await api.previewChart({
            element: draft.element,
            metric: draft.metric,
            grouping: draft.grouping,
            filters: draft.filters,
          }),
        )
        setPreviewErr('')
      } catch (e) {
        setPreview(null)
        setPreviewErr(msg(e))
      }
    }, 350)
    return () => clearTimeout(h)
  }, [draft.element, draft.metric, draft.grouping, draft.filters])

  /// Switching element re-derives everything downstream of it, because a
  /// contact has no `dealQuantity` and an organization has no `role`. Anything
  /// still valid is kept; anything that isn't would only produce a 400 on save.
  function changeElement(key: string) {
    setDraft((d) => {
      const el = schema.elements.find((e) => e.key === key)
      if (!el) return d
      return {
        ...d,
        element: key,
        metric: el.metrics.some((m) => m.key === d.metric) ? d.metric : (el.metrics[0]?.key ?? 'count'),
        grouping: el.fields.some((f) => f.key === d.grouping && f.groupable) ? d.grouping : '',
        // A filter survives only if the new element has that field AND the
        // field still accepts the operator — `kind` is an enum on both
        // organization and service, but `amount` is a number and `stage` isn't.
        filters: d.filters.filter((f) => {
          const def = el.fields.find((x) => x.key === f.field)
          return !!def && def.operators.includes(f.op)
        }),
      }
    })
  }

  function patchFilter(i: number, patch: Partial<ChartFilter>) {
    setDraft((d) => ({ ...d, filters: d.filters.map((f, j) => (j === i ? { ...f, ...patch } : f)) }))
  }

  function addFilter() {
    const def = element?.fields[0]
    if (!def) return
    setDraft((d) => ({ ...d, filters: [...d.filters, { field: def.key, op: def.operators[0]!, values: [] }] }))
  }

  function setColor(i: number, hex: string) {
    setDraft((d) => {
      const rows = preview?.rows ?? []
      // Materialise the colours currently on screen before overriding one, so
      // setting swatch 3 doesn't leave 1 and 2 undefined.
      const next = rows
        .slice(0, MAX_COLOR_SWATCHES)
        .map((r, j) => d.display.colors?.[j] ?? defaultBucketColor(d.element, d.grouping, r.bucket, j))
      next[i] = hex
      return { ...d, display: { ...d.display, colors: next } }
    })
  }

  async function save() {
    if (!draft.name.trim()) {
      setErr(t('chartNameRequired'))
      return
    }
    setBusy(true)
    setErr('')
    try {
      const body: Partial<ChartInput> = { ...draft, name: draft.name.trim() }
      if (chart) await api.updateChart(chart.id, body)
      else await api.createChart(body)
      onSaved()
    } catch (e) {
      // The server rejects an invalid combination with a readable reason
      // ("element 'contact' has no metric 'dealQuantity'") — show it verbatim
      // rather than inventing a vaguer one.
      setErr(msg(e))
    } finally {
      setBusy(false)
    }
  }

  const groupable = element?.fields.filter((f) => f.groupable) ?? []

  const configuration = (
    <div className="builder-section">
      <label className="builder-field">
        <span className="builder-label">
          {t('chartName')} <b className="req">*</b>
        </span>
        <Input
          value={draft.name}
          onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
          placeholder={t('chartNamePh')}
        />
      </label>

      <label className="builder-field">
        <span className="builder-label">
          {t('chartElement')} <b className="req">*</b>
        </span>
        <Select
          value={draft.element}
          onChange={changeElement}
          options={schema.elements.map((e) => ({ value: e.key, label: tk(t, 'el', e.key) }))}
        />
      </label>

      <label className="builder-field">
        <span className="builder-label">
          {t('chartMetric')} <b className="req">*</b>
        </span>
        <Select
          value={draft.metric}
          onChange={(v) => setDraft((d) => ({ ...d, metric: v }))}
          // The metric is named against the SELECTED element — the reference
          // says "Count Contact" even when the element is Organization.
          options={(element?.metrics ?? []).map((m) => ({
            value: m.key,
            label: fmt(tk(t, 'metricLabel', m.key), { a: tk(t, 'el', draft.element) }),
          }))}
        />
      </label>

      <label className="builder-field">
        <span className="builder-label">{t('chartGrouping')}</span>
        <Select
          value={draft.grouping}
          onChange={(v) => setDraft((d) => ({ ...d, grouping: v }))}
          options={[
            { value: '', label: t('noGrouping') },
            ...groupable.map((f) => ({ value: f.key, label: tk(t, 'fld', f.key) })),
          ]}
        />
      </label>

      <label className="builder-field row">
        <Switch
          checked={draft.is_template}
          onChange={(v) => setDraft((d) => ({ ...d, is_template: v }))}
          size="small"
        />
        <span>{t('shareAsTemplate')}</span>
      </label>
    </div>
  )

  const filtersSection = (
    <div className="builder-section">
      {draft.filters.length === 0 && <div className="muted small">{t('noFiltersYet')}</div>}
      {draft.filters.map((f, i) => (
        <FilterRow
          key={i}
          t={t}
          filter={f}
          element={element}
          elementKey={draft.element}
          values={values}
          loadValues={loadValues}
          onChange={(patch) => patchFilter(i, patch)}
          onRemove={() => setDraft((d) => ({ ...d, filters: d.filters.filter((_, j) => j !== i) }))}
        />
      ))}
      <Button size="small" icon={<PlusOutlined />} onClick={addFilter} disabled={!element}>
        {t('addFilter')}
      </Button>
    </div>
  )

  const displaySection = (
    <div className="builder-section">
      <label className="builder-field">
        <span className="builder-label">{t('chartType')}</span>
        <Select
          value={draft.display.type ?? schema.displayTypes[0]}
          onChange={(v) => setDraft((d) => ({ ...d, display: { ...d.display, type: v } }))}
          options={schema.displayTypes.map((x) => ({ value: x, label: tk(t, 'dt', x) }))}
        />
      </label>

      <label className="builder-field">
        <span className="builder-label">{t('chartSize')}</span>
        <Select
          value={draft.size}
          onChange={(v) => setDraft((d) => ({ ...d, size: v }))}
          options={schema.sizes.map((x) => ({ value: x, label: tk(t, 'chartSize', x) }))}
        />
      </label>

      <div className="builder-checks">
        <Checkbox
          checked={!!draft.display.showFilters}
          onChange={(e) => setDraft((d) => ({ ...d, display: { ...d.display, showFilters: e.target.checked } }))}
        >
          {t('showFiltersLabel')}
        </Checkbox>
        <Checkbox
          checked={!!draft.display.reverseX}
          onChange={(e) => setDraft((d) => ({ ...d, display: { ...d.display, reverseX: e.target.checked } }))}
        >
          {t('reverseXLabel')}
        </Checkbox>
        <Checkbox
          checked={!!draft.display.reverseY}
          onChange={(e) => setDraft((d) => ({ ...d, display: { ...d.display, reverseY: e.target.checked } }))}
        >
          {t('reverseYLabel')}
        </Checkbox>
      </div>

      {/* Swatches follow the preview's actual buckets, so the operator colours
          the series they can see rather than an abstract index. */}
      {!!draft.grouping && !!preview && preview.rows.length > 0 && (
        <div className="builder-field">
          <span className="builder-label">{t('seriesColors')}</span>
          <div className="builder-colors">
            {preview.rows.slice(0, MAX_COLOR_SWATCHES).map((r, i) => (
              // The swatch alone doesn't say which bucket it paints.
              <Tooltip key={`${r.bucket}-${i}`} title={chartBucketLabel(t, draft.element, draft.grouping, r.bucket)}>
                <span>
                  <ColorPicker
                    size="small"
                    disabledAlpha
                    value={
                      draft.display.colors?.[i] ?? defaultBucketColor(draft.element, draft.grouping, r.bucket, i)
                    }
                    onChangeComplete={(c) => setColor(i, c.toHexString())}
                  />
                </span>
              </Tooltip>
            ))}
            <Button
              size="small"
              type="text"
              onClick={() => setDraft((d) => ({ ...d, display: { ...d.display, colors: undefined } }))}
            >
              {t('autoColors')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )

  return (
    <Modal
      open
      title={chart ? t('editChart') : t('newChart')}
      onCancel={onClose}
      width={900}
      className="chart-builder-modal"
      footer={[
        <Button key="reset" onClick={() => setDraft(initial)}>
          {t('reset')}
        </Button>,
        <Button key="cancel" onClick={onClose}>
          {t('cancel')}
        </Button>,
        <Button key="save" type="primary" loading={busy} onClick={save}>
          {t('save')}
        </Button>,
      ]}
    >
      <div className="builder-grid">
        <div className="builder-form">
          <Collapse
            defaultActiveKey={['config', 'filters', 'display']}
            items={[
              { key: 'config', label: t('configuration'), children: configuration },
              {
                key: 'filters',
                label: fmt(t('filtersSection'), { a: tk(t, 'el', draft.element) }),
                extra: draft.filters.length > 0 ? <span className="chart-badge">{draft.filters.length}</span> : null,
                children: filtersSection,
              },
              { key: 'display', label: t('displaySection'), children: displaySection },
            ]}
          />
        </div>

        <div className="builder-preview">
          <div className="section-title">{t('previewTitle')}</div>
          <div className="card builder-preview-card">
            {previewErr ? (
              <div className="chart-err">
                <div className="chart-err-msg">{previewErr}</div>
              </div>
            ) : preview ? (
              <ChartBody
                element={draft.element}
                grouping={draft.grouping}
                display={draft.display}
                data={preview}
                t={t}
              />
            ) : (
              <div className="empty small">{t('loading')}</div>
            )}
          </div>
          <div className="muted small">{t('previewHint')}</div>
          {err && <div className="err inline">{err}</div>}
        </div>
      </div>
    </Modal>
  )
}

/// One `field → operator → value(s)` row. The operator list and the value
/// editor both come off the field's `kind`, exactly as the registry declares
/// it, so the UI can't offer a combination the backend would reject.
function FilterRow({
  t,
  filter,
  element,
  elementKey,
  values,
  loadValues,
  onChange,
  onRemove,
}: {
  t: T
  filter: ChartFilter
  element: DashElement | undefined
  elementKey: string
  values: Record<string, string[]>
  loadValues: (element: string, field: string) => void
  onChange: (patch: Partial<ChartFilter>) => void
  onRemove: () => void
}) {
  const def: DashField | undefined = element?.fields.find((f) => f.key === filter.field)

  // Only open sets need a round-trip; a fixed vocabulary is already in the
  // schema we were handed.
  useEffect(() => {
    if (def && def.values.length === 0 && (def.kind === 'enum' || def.kind === 'relation' || def.kind === 'text')) {
      loadValues(elementKey, def.key)
    }
  }, [def, elementKey, loadValues])

  function changeField(key: string) {
    const next = element?.fields.find((f) => f.key === key)
    // A new field means a new kind, so the old operator and operands rarely
    // survive; reset to the field's first legal operator.
    onChange({ field: key, op: next?.operators[0] ?? 'in', values: [] })
  }

  function changeOp(op: string) {
    // in↔notIn keep their operand list — every other switch changes arity.
    const sameFamily = ['in', 'notIn'].includes(op) && ['in', 'notIn'].includes(filter.op)
    onChange({ op, values: sameFamily ? filter.values : [] })
  }

  return (
    <div className="filter-row">
      <Select
        className="filter-field"
        value={filter.field}
        onChange={changeField}
        options={(element?.fields ?? []).map((f) => ({ value: f.key, label: tk(t, 'fld', f.key) }))}
        placeholder={t('filterField')}
      />
      <Select
        className="filter-op"
        value={filter.op}
        onChange={changeOp}
        options={(def?.operators ?? []).map((o) => ({ value: o, label: tk(t, 'op', o) }))}
        placeholder={t('filterOperator')}
      />
      <div className="filter-value">
        <FilterValue t={t} filter={filter} def={def} elementKey={elementKey} values={values} onChange={onChange} />
      </div>
      <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={onRemove} />
    </div>
  )
}

function FilterValue({
  t,
  filter,
  def,
  elementKey,
  values,
  onChange,
}: {
  t: T
  filter: ChartFilter
  def: DashField | undefined
  elementKey: string
  values: Record<string, string[]>
  onChange: (patch: Partial<ChartFilter>) => void
}) {
  if (!def) return null
  const { op } = filter

  // isNull/isNotNull are complete on their own.
  if (op === 'isNull' || op === 'isNotNull') return <span className="muted small">—</span>

  if (op === 'inLastDays') {
    return (
      <span className="filter-days">
        <InputNumber
          min={1}
          value={typeof filter.values[0] === 'number' ? filter.values[0] : undefined}
          onChange={(v) => onChange({ values: v == null ? [] : [v] })}
        />
        <span className="muted small">{t('daysUnit')}</span>
      </span>
    )
  }

  if (def.kind === 'date') {
    if (op === 'between') {
      const from = typeof filter.values[0] === 'number' ? dayjs.unix(filter.values[0]) : null
      const to = typeof filter.values[1] === 'number' ? dayjs.unix(filter.values[1]) : null
      return (
        <DatePicker.RangePicker
          value={from && to ? [from, to] : null}
          onChange={(d) => onChange({ values: d?.[0] && d?.[1] ? [d[0].unix(), d[1].unix()] : [] })}
        />
      )
    }
    const cur = typeof filter.values[0] === 'number' ? dayjs.unix(filter.values[0]) : null
    return <DatePicker value={cur} onChange={(d) => onChange({ values: d ? [d.unix()] : [] })} />
  }

  if (def.kind === 'number') {
    if (op === 'between') {
      return (
        <span className="filter-between">
          <InputNumber
            placeholder={t('from')}
            value={typeof filter.values[0] === 'number' ? filter.values[0] : undefined}
            onChange={(v) => onChange({ values: [v ?? 0, filter.values[1] ?? 0] })}
          />
          <InputNumber
            placeholder={t('to')}
            value={typeof filter.values[1] === 'number' ? filter.values[1] : undefined}
            onChange={(v) => onChange({ values: [filter.values[0] ?? 0, v ?? 0] })}
          />
        </span>
      )
    }
    return (
      <InputNumber
        style={{ width: '100%' }}
        value={typeof filter.values[0] === 'number' ? filter.values[0] : undefined}
        onChange={(v) => onChange({ values: v == null ? [] : [v] })}
      />
    )
  }

  // enum / bool / relation / text — all of them filter by set membership.
  // A fixed vocabulary comes from the schema; an open set was fetched.
  const opts = def.values.length > 0 ? def.values : (values[`${elementKey}.${def.key}`] ?? [])
  return (
    <Select
      // Open sets let you name a value that isn't in the data yet; a fixed
      // vocabulary must not.
      mode={def.kind === 'text' ? 'tags' : 'multiple'}
      style={{ width: '100%' }}
      value={filter.values.map(String)}
      onChange={(v: string[]) => onChange({ values: v })}
      options={opts.map((v) => ({ value: v, label: chartBucketLabel(t, elementKey, def.key, v) }))}
      placeholder={t('pickValues')}
      allowClear
    />
  )
}
