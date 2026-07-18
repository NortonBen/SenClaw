import { useCallback, useEffect, useState } from 'react'
import { Button, InputNumber, Select, Space } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import { api, formatMoney, type DealService, type Service } from '../api'
import { PricingBadge, ServiceKindBadge } from './chips'
import type { T } from '../i18n'

/// Line items on one deal. The server recomputes `deals.amount` from the sum
/// whenever there is at least one line, so `onChanged` refetches the deal.
export function DealServicesEditor({
  dealId,
  currency,
  t,
  onChanged,
  compact,
}: {
  dealId: number
  currency: string
  t: T
  onChanged?: () => void
  compact?: boolean
}) {
  const [items, setItems] = useState<DealService[]>([])
  const [total, setTotal] = useState(0)
  const [catalog, setCatalog] = useState<Service[]>([])
  const [adding, setAdding] = useState(false)
  const [pick, setPick] = useState<number | undefined>()
  const [qty, setQty] = useState<number>(1)
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    const r = await api.dealServices(dealId)
    setItems(r.services)
    setTotal(r.total)
  }, [dealId])

  useEffect(() => {
    refresh()
  }, [refresh])

  useEffect(() => {
    if (adding && catalog.length === 0) api.listServices({ active_only: true, limit: 200 }).then(setCatalog)
  }, [adding, catalog.length])

  async function attach() {
    if (pick == null) return
    setBusy(true)
    try {
      await api.attachService(dealId, { service_id: pick, quantity: qty })
      setAdding(false)
      setPick(undefined)
      setQty(1)
      await refresh()
      onChanged?.()
    } finally {
      setBusy(false)
    }
  }

  async function detach(serviceId: number) {
    if (!confirm(t('detachService'))) return
    await api.detachService(dealId, serviceId)
    await refresh()
    onChanged?.()
  }

  return (
    <div className="line-items">
      {!compact && (
        <div className="section-title">
          🧾 {t('lineItems')} ({items.length})
          <Button
            size="small"
            type="text"
            icon={<PlusOutlined />}
            className="right"
            onClick={() => setAdding(true)}
          >
            {t('addLineItem')}
          </Button>
        </div>
      )}
      {items.length === 0 && !adding && <div className="empty small">{t('noLineItems')}</div>}
      {items.map((it) => (
        <div key={it.id} className="line-item">
          <div className="line-item-main">
            <div className="line-item-name">
              {it.name} <ServiceKindBadge kind={it.kind} t={t} />{' '}
              <PricingBadge model={it.pricing_model} t={t} />
            </div>
            <div className="muted small">
              {it.quantity} × {formatMoney(it.unit_amount, it.currency || currency)}
              {it.note && ` · ${it.note}`}
            </div>
          </div>
          <div className="line-item-total">{formatMoney(it.line_total, it.currency || currency)}</div>
          <button className="tl-del" onClick={() => detach(it.service_id)} title={t('del')}>
            ×
          </button>
        </div>
      ))}
      {items.length > 0 && (
        <div className="line-item total-row">
          <div className="line-item-main">
            <b>{t('dealTotal')}</b>
            <div className="muted small">{t('lineItemsNote')}</div>
          </div>
          <div className="line-item-total strong">{formatMoney(total, currency)}</div>
        </div>
      )}
      {adding ? (
        <Space wrap style={{ marginTop: 8 }}>
          <Select
            showSearch
            style={{ minWidth: 240 }}
            placeholder={t('pickService')}
            value={pick}
            onChange={setPick}
            optionFilterProp="label"
            options={catalog.map((s) => ({
              value: s.id,
              label: `${s.name} — ${formatMoney(s.amount, s.currency)}`,
            }))}
          />
          <InputNumber min={0.01} step={1} value={qty} onChange={(v) => setQty(Number(v) || 1)} />
          <Button size="small" onClick={() => setAdding(false)}>
            {t('cancel')}
          </Button>
          <Button size="small" type="primary" loading={busy} disabled={pick == null} onClick={attach}>
            {t('add')}
          </Button>
        </Space>
      ) : (
        compact && (
          <Button size="small" type="dashed" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
            {t('addLineItem')}
          </Button>
        )
      )}
    </div>
  )
}
