import { useCallback, useEffect, useState } from 'react'
import { Button, Space } from 'antd'
import { DownloadOutlined, PlusOutlined } from '@ant-design/icons'
import {
  api,
  fmtDate,
  fmtDateTime,
  formatMoney,
  type ActivityItem,
  type Chart,
  type ChartCell,
  type DashSchema,
  type Deal,
  type InboxStats,
  type SaleStats,
  type Stats,
  type Upcoming,
} from '../api'
import { KIND_ICONS } from '../constants'
import type { T } from '../i18n'
import { subscribeEvents } from '../events'
import type { View } from '../components/Sidebar'
import { PageShell } from '../components/PageShell'
import { StageBar, StatTile, formatShortMoney } from '../components/StatTile'
import { ChartCard } from '../components/ChartCard'
import { ChartBuilder } from '../components/ChartBuilder'
import { DealStageBadge } from '../components/chips'
import { renderMd } from '../components/Field'

export function DashboardPage({
  stats,
  t,
  onOpenNew,
  onPickCustomer,
  onGoto,
}: {
  stats: Stats | null
  t: T
  onOpenNew: () => void
  onPickCustomer: (id: number) => void
  onGoto: (v: View) => void
}) {
  const [topDeals, setTopDeals] = useState<Deal[]>([])
  const [upcoming, setUpcoming] = useState<Upcoming | null>(null)
  const [activity, setActivity] = useState<ActivityItem[]>([])
  const [sale, setSale] = useState<SaleStats | null>(null)
  const [inbox, setInbox] = useState<InboxStats | null>(null)

  useEffect(() => {
    api.listDeals().then((deals) => {
      const open = deals.filter((d) => d.stage !== 'won' && d.stage !== 'lost')
      open.sort((a, b) => b.amount - a.amount)
      setTopDeals(open.slice(0, 5))
    })
    api.upcoming(14).then(setUpcoming)
    api.activity(5).then(setActivity)
    // These degrade rather than blow up the whole dashboard when an endpoint
    // isn't there yet.
    api.saleStats().then(setSale).catch(() => setSale(null))
    api.inboxStats().then(setInbox).catch(() => setInbox(null))
  }, [])

  return (
    <PageShell
      title={t('navDashboard')}
      subtitle={t('dashboardSub')}
      actions={
        <Space>
          <Button icon={<DownloadOutlined />} href="/api/export.csv" download>
            {t('exportCsv')}
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={onOpenNew}>
            {t('newCustomer')}
          </Button>
        </Space>
      }
    >
      {stats && (
        <div className="stat-grid">
          <StatTile
            label={t('kpiCustomers')}
            value={String(stats.customers)}
            accent="#5e4ae3"
            onClick={() => onGoto('contacts')}
          />
          <StatTile
            label={t('kpiOpenDeals')}
            value={String(stats.open_deals)}
            sub={`${formatMoney(stats.pipeline_value, topDeals[0]?.currency ?? 'VND')} ${t('kpiOpenDealsSub')}`}
            accent="#007aff"
            onClick={() => onGoto('deals')}
          />
          <StatTile
            label={t('kpiWon')}
            value={formatShortMoney(stats.won_value)}
            sub={`${stats.by_stage?.won?.count ?? 0} ${t('kpiWonSub')}`}
            accent="#34c759"
            onClick={() => onGoto('deals')}
          />
          <StatTile
            label={t('kpiOpenTasks')}
            value={String(stats.open_tasks)}
            sub={stats.overdue_tasks > 0 ? `${stats.overdue_tasks} ${t('kpiOverdue')}` : t('kpiOnTime')}
            accent={stats.overdue_tasks > 0 ? '#ff9500' : '#34c759'}
            warn={stats.overdue_tasks > 0}
            onClick={() => onGoto('tasks')}
          />
          <StatTile
            label={t('kpiInteractions')}
            value={String(stats.interactions)}
            sub={t('kpiTotal')}
            accent="#af52de"
            onClick={() => onGoto('activity')}
          />
        </div>
      )}

      {/* sales + inbox counters */}
      {(sale || inbox) && (
        <div className="stat-grid">
          {sale && (
            <>
              <StatTile
                label={t('winRate')}
                value={`${sale.winRate}%`}
                sub={`${sale.won} / ${sale.won + sale.churned}`}
                accent="#34c759"
                onClick={() => onGoto('pipeline')}
              />
              <StatTile
                label={t('hotLeads')}
                value={String(sale.hotLeads)}
                accent="#ff3b30"
                onClick={() => onGoto('pipeline')}
              />
              <StatTile
                label={t('pendingReviews')}
                value={String(sale.pendingReviews)}
                accent="#ff9500"
                warn={sale.pendingReviews > 0}
                onClick={() => onGoto('reviews')}
              />
              <StatTile
                label={t('openEscalations')}
                value={String(sale.openEscalations)}
                accent="#ff3b30"
                warn={sale.openEscalations > 0}
                onClick={() => onGoto('escalations')}
              />
            </>
          )}
          {inbox && (
            <StatTile
              label={t('inboxCounters')}
              value={String(inbox.unread)}
              sub={`${inbox.openConversations} ${t('openConversations').toLowerCase()} · ${inbox.unlinked} ${t(
                'unlinked',
              ).toLowerCase()}`}
              accent="#007aff"
              warn={inbox.unread > 0}
              onClick={() => onGoto('inbox')}
            />
          )}
        </div>
      )}

      {stats && Object.keys(stats.by_stage ?? {}).length > 0 && (
        <div className="card">
          <div className="section-title">🗂 {t('pipelineByStage')}</div>
          <StageBar byStage={stats.by_stage} t={t} />
        </div>
      )}

      <AggregateReportCard t={t} />

      <div className="dash-grid">
        {/* "Tổ chức theo loại", "Giá trị deal theo tổ chức", "Giá trị deal theo
            loại" and "Phễu bán hàng" used to be hand-built here. They are now
            seeded as real charts in the dynamic grid below — keeping a second
            hardcoded copy would render each of them twice. */}
        <div className="card">
          <div className="section-title">
            🔥 {t('topOpenDeals')}
            <button className="linklike right" onClick={() => onGoto('deals')}>
              {t('viewAll')}
            </button>
          </div>
          {topDeals.length === 0 && <div className="empty small">{t('noDeals')}</div>}
          {topDeals.map((d) => (
            <div
              key={d.id}
              className="deal-row"
              onClick={() => onPickCustomer(d.customer_id)}
              style={{ cursor: 'pointer' }}
            >
              <div className="deal-row-main">
                <div className="deal-title">{d.title}</div>
                <div className="deal-sub">
                  {d.customer_name} · {formatMoney(d.amount, d.currency)} · {d.probability}%
                </div>
              </div>
              <DealStageBadge stage={d.stage} t={t} />
            </div>
          ))}
        </div>

        <div className="card">
          <div className="section-title">
            🎂 {t('upcoming14')}
            <button className="linklike right" onClick={() => onGoto('tasks')}>
              {t('viewAll')}
            </button>
          </div>
          {upcoming && upcoming.birthdays.length === 0 && upcoming.tasks.length === 0 && (
            <div className="empty small">{t('noEvents')}</div>
          )}
          {upcoming?.birthdays.slice(0, 4).map((b) => (
            <div
              key={'b' + b.customer_id}
              className="upcoming-row"
              onClick={() => onPickCustomer(b.customer_id)}
              style={{ cursor: 'pointer' }}
            >
              <span className="upcoming-icon">🎂</span>
              <div>
                <b>{b.customer_name}</b>
                <div className="task-sub">
                  {t('birthdayLabel')} · {fmtDate(b.next_at)}
                </div>
              </div>
            </div>
          ))}
          {upcoming?.tasks.slice(0, 4).map((task) => (
            <div
              key={'t' + task.id}
              className="upcoming-row"
              onClick={() => task.customer_id && onPickCustomer(task.customer_id)}
              style={{ cursor: task.customer_id ? 'pointer' : 'default' }}
            >
              <span className="upcoming-icon">📌</span>
              <div>
                <b>{task.title}</b>
                <div className="task-sub">
                  {t('dueLabel')} {fmtDate(task.due_at)} {task.customer_name && `· ${task.customer_name}`}
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="card wide">
          <div className="section-title">
            🕐 {t('recentActivity')}
            <button className="linklike right" onClick={() => onGoto('activity')}>
              {t('viewAll')}
            </button>
          </div>
          {activity.length === 0 && <div className="empty small">{t('noInteractions')}</div>}
          <div className="timeline compact">
            {activity.map((i) => (
              <div className="tl-item" key={i.id}>
                <div className="tl-dot">{KIND_ICONS[i.kind] ?? '•'}</div>
                <div className="tl-body">
                  <div className="tl-head">
                    <span className="tl-summary">
                      <button className="linklike" onClick={() => onPickCustomer(i.customer_id)}>
                        {i.customer_name}
                      </button>
                      {' — '}
                      {i.summary}
                    </span>
                    <span className="tl-when">{fmtDateTime(i.occurred_at)}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <ChartGrid t={t} />
    </PageShell>
  )
}

/// The user-defined chart grid. Order is drag-and-drop, and each chart claims
/// one, two or three columns by its size.
function ChartGrid({ t }: { t: T }) {
  const [cells, setCells] = useState<ChartCell[]>([])
  const [schema, setSchema] = useState<DashSchema | null>(null)
  /// `undefined` = the builder is closed, `null` = open on a new chart.
  const [editing, setEditing] = useState<Chart | null | undefined>(undefined)
  const [err, setErr] = useState('')
  const [dragId, setDragId] = useState<number | null>(null)
  const [overId, setOverId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    try {
      // One round-trip for every chart AND its data — the endpoint resolves
      // them server-side, so there is no N+1 here.
      setCells(await api.listCharts())
      setErr('')
    } catch (e) {
      setErr(t('chartsLoadFailed') + (e instanceof Error ? e.message : String(e)))
    }
  }, [t])

  useEffect(() => {
    api.dashboardSchema().then(setSchema).catch(() => setSchema(null))
  }, [])
  useEffect(() => {
    refresh()
  }, [refresh])

  // Any CRM write can move these numbers, so ride the shared event stream
  // rather than opening another one. Debounced: a burst of writes should cost
  // one recompute, not one per event.
  useEffect(() => {
    let h: ReturnType<typeof setTimeout> | undefined
    const off = subscribeEvents(() => {
      clearTimeout(h)
      h = setTimeout(refresh, 800)
    })
    return () => {
      clearTimeout(h)
      off()
    }
  }, [refresh])

  /// Move the dragged chart in front of the drop target and persist the whole
  /// order. Applied locally first so the card lands where it was dropped
  /// instead of snapping back for a round-trip.
  function drop(targetId: number) {
    if (dragId == null || dragId === targetId) return
    const ids = cells.map((c) => c.chart.id)
    const from = ids.indexOf(dragId)
    const to = ids.indexOf(targetId)
    if (from < 0 || to < 0) return
    ids.splice(to, 0, ids.splice(from, 1)[0]!)
    const byId = new Map(cells.map((c) => [c.chart.id, c]))
    setCells(ids.map((id) => byId.get(id)!))
    api.reorderCharts(ids).catch((e) => {
      setErr(e instanceof Error ? e.message : String(e))
      refresh()
    })
  }

  async function duplicate(c: Chart) {
    try {
      await api.createChart({
        name: `${c.name} (${t('copySuffix')})`,
        element: c.element,
        metric: c.metric,
        grouping: c.grouping,
        filters: c.filters,
        display: c.display,
        size: c.size,
        is_template: c.is_template,
      })
      await refresh()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    }
  }

  async function remove(id: number) {
    if (!confirm(t('deleteChartConfirm'))) return
    try {
      await api.deleteChart(id)
      await refresh()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <>
      <div className="section-title chartgrid-head">
        📊 {t('chartsSection')}
        <span className="chartgrid-hint">{t('chartsSectionSub')}</span>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          className="right"
          onClick={() => setEditing(null)}
          disabled={!schema}
        >
          {t('addChart')}
        </Button>
      </div>

      {err && <div className="err inline">{err}</div>}

      {cells.length === 0 ? (
        <div className="card">
          <div className="empty small">{t('noCharts')}</div>
        </div>
      ) : (
        <div className="chart-grid">
          {cells.map((cell) => (
            <div
              key={cell.chart.id}
              className={
                'chart-cell ' +
                cell.chart.size +
                (dragId === cell.chart.id ? ' dragging' : '') +
                (overId === cell.chart.id && dragId !== cell.chart.id ? ' dragover' : '')
              }
              draggable
              onDragStart={() => setDragId(cell.chart.id)}
              onDragEnd={() => {
                setDragId(null)
                setOverId(null)
              }}
              onDragOver={(e) => {
                // Without this the drop event never fires at all.
                e.preventDefault()
                setOverId(cell.chart.id)
              }}
              onDragLeave={() => setOverId((o) => (o === cell.chart.id ? null : o))}
              onDrop={(e) => {
                e.preventDefault()
                drop(cell.chart.id)
                setOverId(null)
              }}
            >
              <ChartCard
                cell={cell}
                schema={schema}
                t={t}
                onEdit={() => setEditing(cell.chart)}
                onDuplicate={() => duplicate(cell.chart)}
                onDelete={() => remove(cell.chart.id)}
              />
            </div>
          ))}
        </div>
      )}

      {editing !== undefined && schema && (
        <ChartBuilder
          chart={editing}
          schema={schema}
          t={t}
          onClose={() => setEditing(undefined)}
          onSaved={() => {
            setEditing(undefined)
            refresh()
          }}
        />
      )}
    </>
  )
}

function AggregateReportCard({ t }: { t: T }) {
  const [report, setReport] = useState<{
    text: string
    model: string
    generated_at: number
    grounding: {
      customers: number
      open_deals: number
      pipeline_value: number
      top_deals: number
      recent_events: number
      overdue_tasks: number
    }
  } | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function run() {
    setBusy(true)
    setErr('')
    try {
      setReport(await api.aggregateReport())
    } catch (e) {
      setErr(t('needLlm') + String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="card ai report">
      <div className="section-title">
        ✨ {t('aiReport')}
        <Button size="small" type="primary" className="right" loading={busy} onClick={run}>
          {busy ? t('aiReportBusy') : report ? t('aiReportAgain') : t('aiReportRun')}
        </Button>
      </div>
      {!report && !err && !busy && <div className="muted small">{t('aiReportHint')}</div>}
      {report && (
        <>
          <div className="ai-out markdown" dangerouslySetInnerHTML={{ __html: renderMd(report.text) }} />
          <div className="report-foot">
            <span>
              📊 {report.grounding.customers} · {report.grounding.open_deals} · {report.grounding.overdue_tasks}
            </span>
            <span className="report-meta">
              {report.model} · {fmtDateTime(report.generated_at)}
            </span>
          </div>
        </>
      )}
      {err && <div className="err inline">{err}</div>}
    </div>
  )
}
