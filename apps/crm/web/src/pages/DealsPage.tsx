import { useCallback, useEffect, useState } from 'react'
import { Button, Drawer, Space } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { api, fmtDate, formatMoney, type Deal, type DealServices } from '../api'
import { STAGE_COLORS, STAGE_ORDER } from '../constants'
import { tk, type T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { Chip } from '../components/chips'
import { EditDealForm } from '../components/contact/ContactDeals'

/// Deals as a kanban grouped by stage. Cards show value, service quantity,
/// project period, the org chip and service chips.
export function DealsPage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [deals, setDeals] = useState<Deal[]>([])
  const [dragging, setDragging] = useState<number | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
  const [lines, setLines] = useState<Record<number, DealServices>>({})
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const rows = await api.listDeals()
      setDeals(rows)
      // Line items are per-deal; fetch them alongside so a card can show its
      // service chips + quantity without the board doing it lazily on hover.
      const entries = await Promise.all(
        rows.map(async (d) => [d.id, await api.dealServices(d.id).catch(() => null)] as const),
      )
      const map: Record<number, DealServices> = {}
      for (const [id, v] of entries) if (v) map[id] = v
      setLines(map)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function drop(dealId: number, stage: string) {
    setDragging(null)
    await api.updateDeal(dealId, { stage })
    await refresh()
  }

  const open = deals.find((d) => d.id === openId) ?? null

  return (
    <PageShell
      title={t('navDeals')}
      subtitle={`${deals.length} ${t('deals').toLowerCase()}`}
      actions={
        <Button icon={<ReloadOutlined />} loading={loading} onClick={refresh}>
          {t('refresh')}
        </Button>
      }
    >
      <div className="pipeline">
        {STAGE_ORDER.map((stage) => {
          const col = deals.filter((d) => d.stage === stage)
          const total = col.reduce((s, d) => s + d.amount, 0)
          return (
            <div
              key={stage}
              className={'kanban-col stage-' + stage}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault()
                if (dragging != null) drop(dragging, stage)
              }}
            >
              <div className="kanban-head" style={{ borderTopColor: STAGE_COLORS[stage] }}>
                <span>{tk(t, 'dealStage', stage)}</span>
                <span className="kanban-count">{col.length}</span>
              </div>
              <div className="kanban-sub">{formatMoney(total, col[0]?.currency ?? 'VND')}</div>
              <div className="kanban-cards">
                {col.map((d) => {
                  const li = lines[d.id]
                  // `organization_name` rides along on the deal, so the org chip
                  // needs no second lookup. 0 / '' means the deal is unlinked.
                  const orgName = d.organization_id ? d.organization_name : ''
                  const period = dealPeriod(d)
                  return (
                    <div
                      key={d.id}
                      className="kanban-card"
                      draggable
                      onDragStart={() => setDragging(d.id)}
                      onDragEnd={() => setDragging(null)}
                      onClick={() => setOpenId(d.id)}
                    >
                      <div className="kc-title">{d.title}</div>
                      <button
                        className="kc-customer linklike"
                        onClick={(e) => {
                          e.stopPropagation()
                          onPickCustomer(d.customer_id)
                        }}
                      >
                        {d.customer_name}
                      </button>
                      <div className="kc-amount">
                        {formatMoney(d.amount, d.currency)} · {d.probability}%
                      </div>
                      <div className="kc-chips">
                        {orgName && <Chip color="#5e4ae3">🏢 {orgName}</Chip>}
                        {li && li.quantity > 0 && (
                          <Chip color="#007aff">
                            × {li.quantity} {t('serviceQty')}
                          </Chip>
                        )}
                      </div>
                      {li && li.services.length > 0 && (
                        <div className="kc-chips">
                          {li.services.slice(0, 3).map((s) => (
                            <Chip key={s.id} color="#8e8e93" title={s.name}>
                              {s.name}
                            </Chip>
                          ))}
                          {li.services.length > 3 && <Chip color="#8e8e93">+{li.services.length - 3}</Chip>}
                        </div>
                      )}
                      {period && <div className="kc-close">🗓 {period}</div>}
                      {d.expected_close_at && <div className="kc-close">📅 {fmtDate(d.expected_close_at)}</div>}
                    </div>
                  )
                })}
                {col.length === 0 && <div className="kanban-empty">—</div>}
              </div>
            </div>
          )
        })}
      </div>
      {deals.length === 0 && <div className="pipeline-hint">{t('noDealsHint')}</div>}

      <Drawer
        open={open != null}
        onClose={() => setOpenId(null)}
        width={620}
        title={open?.title}
        extra={
          open && (
            <Space>
              <Button size="small" onClick={() => onPickCustomer(open.customer_id)}>
                {open.customer_name}
              </Button>
            </Space>
          )
        }
      >
        {open && (
          <EditDealForm
            deal={open}
            t={t}
            onClose={() => setOpenId(null)}
            onSaved={async () => {
              setOpenId(null)
              await refresh()
            }}
            onLinesChanged={refresh}
          />
        )}
      </Drawer>
    </PageShell>
  )
}

/// The reference CRM's "Project Period". Either end may be open.
function dealPeriod(d: Deal): string | null {
  if (!d.period_start && !d.period_end) return null
  const a = d.period_start ? fmtDate(d.period_start) : '…'
  const b = d.period_end ? fmtDate(d.period_end) : '…'
  return `${a} → ${b}`
}
