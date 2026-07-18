import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Button, Input, InputNumber, Modal, Select, Space, Switch } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import { api, fmtDateTime, formatMoney, type Service, type ServiceInput } from '../api'
import { SERVICE_KINDS, SERVICE_KIND_COLORS } from '../constants'
import { tk, type T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { DataTable } from '../components/DataTable'
import { Chip, FilterChips, PricingBadge, ServiceKindBadge, pricingOptions } from '../components/chips'
import { Field } from '../components/Field'

export function ServicesPage({ t }: { t: T }) {
  const [services, setServices] = useState<Service[]>([])
  const [q, setQ] = useState('')
  const [kind, setKind] = useState<string | null>(null)
  const [activeOnly, setActiveOnly] = useState(false)
  const [editing, setEditing] = useState<Service | null>(null)
  const [showNew, setShowNew] = useState(false)
  const [loading, setLoading] = useState(false)
  const { message } = AntApp.useApp()

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setServices(
        await api.listServices({
          q: q || undefined,
          kind: kind ?? undefined,
          active_only: activeOnly || undefined,
          limit: 500,
        }),
      )
    } finally {
      setLoading(false)
    }
  }, [q, kind, activeOnly])

  useEffect(() => {
    const h = setTimeout(refresh, 200)
    return () => clearTimeout(h)
  }, [refresh])

  /// DELETE answers 400 + "…deactivate it instead of deleting" when the entry
  /// priced a real deal. Surface that message verbatim and offer the toggle.
  async function del(s: Service) {
    if (!confirm(t('confirmDelete'))) return
    try {
      await api.deleteService(s.id)
      message.success(t('serviceDeleted'))
      await refresh()
    } catch (e) {
      const text = e instanceof Error ? e.message : String(e)
      message.error({ content: text, duration: 6 })
      Modal.confirm({
        title: t('deactivateInstead'),
        content: text,
        okText: t('inactive'),
        cancelText: t('cancel'),
        onOk: async () => {
          await api.updateService(s.id, { active: false })
          await refresh()
        },
      })
    }
  }

  async function toggleActive(s: Service, active: boolean) {
    await api.updateService(s.id, { active })
    await refresh()
  }

  return (
    <PageShell
      title={t('navServices')}
      subtitle={`${services.length} ${t('services').toLowerCase()}`}
      search={q}
      onSearch={setQ}
      searchPlaceholder={t('searchServices')}
      filters={
        <Space>
          <FilterChips
            value={kind}
            onChange={setKind}
            allLabel={t('all')}
            options={SERVICE_KINDS.map((k) => ({
              value: k,
              color: SERVICE_KIND_COLORS[k] ?? '#8e8e93',
              label: tk(t, 'svcKind', k),
            }))}
          />
          <span className="muted small">
            <Switch size="small" checked={activeOnly} onChange={setActiveOnly} /> {t('activeOnly')}
          </span>
        </Space>
      }
      actions={
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowNew(true)}>
          {t('add')}
        </Button>
      }
    >
      <div className="card flush">
        <DataTable<Service>
          t={t}
          loading={loading}
          dataSource={services}
          locale={{ emptyText: t('noServices') }}
          onRow={(r) => ({ onClick: () => setEditing(r), style: { cursor: 'pointer' } })}
          columns={[
            {
              title: t('name'),
              dataIndex: 'name',
              sorter: (a, b) => a.name.localeCompare(b.name),
              render: (_, r) => (
                <div>
                  <div className="cell-strong">{r.name}</div>
                  {(r.sku || r.unit) && (
                    <div className="muted small">
                      {r.sku}
                      {r.sku && r.unit && ' · '}
                      {r.unit}
                    </div>
                  )}
                </div>
              ),
            },
            {
              title: t('type'),
              dataIndex: 'kind',
              width: 120,
              sorter: (a, b) => a.kind.localeCompare(b.kind),
              render: (k: string) => <ServiceKindBadge kind={k} t={t} />,
            },
            {
              title: t('amount'),
              dataIndex: 'amount',
              width: 150,
              align: 'right',
              sorter: (a, b) => a.amount - b.amount,
              render: (_, r) => <span className="cell-strong">{formatMoney(r.amount, r.currency)}</span>,
            },
            {
              title: t('pricingModel'),
              dataIndex: 'pricing_model',
              width: 130,
              sorter: (a, b) => a.pricing_model.localeCompare(b.pricing_model),
              render: (m: string) => <PricingBadge model={m} t={t} />,
            },
            {
              title: t('dealCount'),
              dataIndex: 'deal_count',
              width: 90,
              sorter: (a, b) => a.deal_count - b.deal_count,
              render: (n: number) => (n > 0 ? <Chip color="#5e4ae3">{n}</Chip> : <span className="muted">—</span>),
            },
            {
              title: t('active'),
              dataIndex: 'active',
              width: 90,
              render: (v: boolean, r) => (
                <span onClick={(e) => e.stopPropagation()}>
                  <Switch size="small" checked={v} onChange={(on) => toggleActive(r, on)} />
                </span>
              ),
            },
            {
              title: t('updatedAt'),
              dataIndex: 'updated_at',
              width: 160,
              sorter: (a, b) => a.updated_at - b.updated_at,
              render: (v: number) => <span className="muted small">{fmtDateTime(v)}</span>,
            },
            {
              title: '',
              width: 48,
              render: (_, r) => (
                <Button
                  size="small"
                  type="text"
                  danger
                  icon={<DeleteOutlined />}
                  onClick={(e) => {
                    e.stopPropagation()
                    del(r)
                  }}
                />
              ),
            },
          ]}
        />
      </div>

      {(showNew || editing) && (
        <ServiceFormModal
          t={t}
          initial={editing ?? undefined}
          onClose={() => {
            setShowNew(false)
            setEditing(null)
          }}
          onSaved={async () => {
            setShowNew(false)
            setEditing(null)
            await refresh()
          }}
        />
      )}
    </PageShell>
  )
}

function ServiceFormModal({
  t,
  initial,
  onClose,
  onSaved,
}: {
  t: T
  initial?: Service
  onClose: () => void
  onSaved: () => Promise<void>
}) {
  const [form, setForm] = useState<ServiceInput>({
    name: initial?.name ?? '',
    kind: initial?.kind ?? 'service',
    amount: initial?.amount ?? 0,
    currency: initial?.currency ?? 'VND',
    pricing_model: initial?.pricing_model ?? 'fixed',
    unit: initial?.unit ?? '',
    sku: initial?.sku ?? '',
    description: initial?.description ?? '',
    active: initial?.active ?? true,
  })
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  function set<K extends keyof ServiceInput>(k: K, v: ServiceInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function save() {
    if (!form.name?.trim()) {
      setErr(t('serviceNameRequired'))
      return
    }
    setBusy(true)
    setErr('')
    try {
      if (initial) await api.updateService(initial.id, { ...form, name: form.name.trim() })
      else await api.createService({ ...form, name: form.name.trim() })
      await onSaved()
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open
      onCancel={onClose}
      title={initial ? t('editService') : t('addService')}
      width={600}
      footer={[
        <Button key="c" onClick={onClose}>
          {t('cancel')}
        </Button>,
        <Button key="s" type="primary" loading={busy} onClick={save}>
          {t('save')}
        </Button>,
      ]}
    >
      <div className="edit-grid">
        <Field label={`${t('name')} *`}>
          <Input autoFocus value={form.name} onChange={(e) => set('name', e.target.value)} />
        </Field>
        <Field label={t('type')}>
          <Select
            value={form.kind}
            onChange={(v) => set('kind', v)}
            style={{ width: '100%' }}
            options={SERVICE_KINDS.map((k) => ({ value: k, label: tk(t, 'svcKind', k) }))}
          />
        </Field>
        <Field label={t('amount')}>
          <InputNumber
            value={form.amount}
            onChange={(v) => set('amount', Number(v) || 0)}
            style={{ width: '100%' }}
            min={0}
          />
        </Field>
        <Field label={t('currency')}>
          <Input
            value={form.currency}
            onChange={(e) => set('currency', e.target.value.toUpperCase().slice(0, 4))}
          />
        </Field>
        <Field label={t('pricingModel')}>
          <Select
            value={form.pricing_model}
            onChange={(v) => set('pricing_model', v)}
            style={{ width: '100%' }}
            options={pricingOptions(t)}
          />
        </Field>
        <Field label={t('unit')}>
          <Input value={form.unit} onChange={(e) => set('unit', e.target.value)} placeholder="user, hour…" />
        </Field>
        <Field label={t('sku')}>
          <Input value={form.sku} onChange={(e) => set('sku', e.target.value)} />
        </Field>
        <Field label={t('active')}>
          <Switch checked={form.active} onChange={(v) => set('active', v)} />
        </Field>
        <Field label={t('description')} full>
          <Input.TextArea
            rows={3}
            value={form.description}
            onChange={(e) => set('description', e.target.value)}
          />
        </Field>
      </div>
      {err && <div className="err inline">{err}</div>}
    </Modal>
  )
}
