import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Button, Drawer, Input, Modal, Select, Space } from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import {
  api,
  fmtDateTime,
  formatMoney,
  type OrgDetail,
  type Organization,
  type OrganizationInput,
} from '../api'
import { ORG_KINDS, ORG_KIND_COLORS } from '../constants'
import { tk, type T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { DataTable } from '../components/DataTable'
import { Avatar } from '../components/Avatar'
import { Chip, DealStageBadge, FilterChips, OrgKindBadge, orgKindOptions } from '../components/chips'
import { Field } from '../components/Field'

export function OrganizationsPage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [orgs, setOrgs] = useState<Organization[]>([])
  const [q, setQ] = useState('')
  const [kind, setKind] = useState<string | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
  const [showNew, setShowNew] = useState(false)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setOrgs(await api.listOrganizations({ q: q || undefined, kind: kind ?? undefined, limit: 500 }))
    } finally {
      setLoading(false)
    }
  }, [q, kind])

  useEffect(() => {
    const h = setTimeout(refresh, 200)
    return () => clearTimeout(h)
  }, [refresh])

  return (
    <PageShell
      title={t('navOrganizations')}
      subtitle={`${orgs.length} ${t('organizations').toLowerCase()}`}
      search={q}
      onSearch={setQ}
      searchPlaceholder={t('searchOrgs')}
      filters={
        <FilterChips
          value={kind}
          onChange={setKind}
          allLabel={t('all')}
          options={ORG_KINDS.map((k) => ({
            value: k,
            color: ORG_KIND_COLORS[k] ?? '#8e8e93',
            label: tk(t, 'orgKind', k),
          }))}
        />
      }
      actions={
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowNew(true)}>
          {t('add')}
        </Button>
      }
    >
      <div className="card flush">
        <DataTable<Organization>
          t={t}
          loading={loading}
          dataSource={orgs}
          locale={{ emptyText: t('noOrgs') }}
          onRow={(r) => ({ onClick: () => setOpenId(r.id), style: { cursor: 'pointer' } })}
          columns={[
            {
              title: t('name'),
              dataIndex: 'name',
              sorter: (a, b) => a.name.localeCompare(b.name),
              render: (_, r) => (
                <div className="cell-org">
                  <Avatar name={r.name} url={r.logo_url} size={28} />
                  <div>
                    <div className="cell-strong">{r.name}</div>
                    {r.domain && <div className="muted small">{r.domain}</div>}
                  </div>
                </div>
              ),
            },
            {
              title: t('orgContacts'),
              dataIndex: 'contact_count',
              width: 120,
              sorter: (a, b) => a.contact_count - b.contact_count,
              render: (n: number) => <OrgContactsCell count={n} />,
            },
            {
              title: t('orgDeals'),
              dataIndex: 'deal_count',
              width: 170,
              sorter: (a, b) => a.open_deal_value - b.open_deal_value,
              render: (n: number, r) => (
                <Space size={4}>
                  <Chip color="#5e4ae3">{n}</Chip>
                  {r.open_deal_value > 0 && (
                    <span className="muted small">{formatMoney(r.open_deal_value, 'VND')}</span>
                  )}
                </Space>
              ),
            },
            {
              title: t('type'),
              dataIndex: 'kind',
              width: 160,
              sorter: (a, b) => a.kind.localeCompare(b.kind),
              render: (k: string) => <OrgKindBadge kind={k} t={t} />,
            },
            {
              title: t('website'),
              dataIndex: 'website',
              width: 180,
              render: (w: string) =>
                w ? (
                  <a href={w.startsWith('http') ? w : `https://${w}`} target="_blank" rel="noopener noreferrer"
                     onClick={(e) => e.stopPropagation()}>
                    {w.replace(/^https?:\/\//, '')}
                  </a>
                ) : (
                  <span className="muted">—</span>
                ),
            },
            {
              title: t('industry'),
              dataIndex: 'industry',
              width: 150,
              sorter: (a, b) => a.industry.localeCompare(b.industry),
              render: (v: string) => v || <span className="muted">—</span>,
            },
          ]}
        />
      </div>

      <OrgDrawer
        id={openId}
        t={t}
        onClose={() => setOpenId(null)}
        onChanged={refresh}
        onPickCustomer={onPickCustomer}
      />
      {showNew && (
        <OrgFormModal
          t={t}
          onClose={() => setShowNew(false)}
          onSaved={async () => {
            setShowNew(false)
            await refresh()
          }}
        />
      )}
    </PageShell>
  )
}

/// The contacts column shows avatars, but the list endpoint only carries a
/// count — so render count-shaped placeholders rather than N+1 fetching.
function OrgContactsCell({ count }: { count: number }) {
  if (count === 0) return <span className="muted">—</span>
  return <Chip color="#007aff">{count}</Chip>
}

function OrgDrawer({
  id,
  t,
  onClose,
  onChanged,
  onPickCustomer,
}: {
  id: number | null
  t: T
  onClose: () => void
  onChanged: () => Promise<void>
  onPickCustomer: (id: number) => void
}) {
  const [d, setD] = useState<OrgDetail | null>(null)
  const [editing, setEditing] = useState(false)
  const { message } = AntApp.useApp()

  const refresh = useCallback(async () => {
    if (id == null) return
    setD(await api.getOrganization(id))
  }, [id])

  useEffect(() => {
    setEditing(false)
    if (id == null) setD(null)
    else refresh()
  }, [id, refresh])

  async function del() {
    if (id == null || !confirm(t('deleteOrg'))) return
    try {
      await api.deleteOrganization(id)
      onClose()
      await onChanged()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    }
  }

  const org = d?.organization
  return (
    <Drawer
      open={id != null}
      onClose={onClose}
      width={560}
      title={
        org && (
          <Space>
            <Avatar name={org.name} url={org.logo_url} size={36} />
            <div>
              <div style={{ fontWeight: 600 }}>{org.name}</div>
              <div className="muted small">{tk(t, 'orgKind', org.kind)}</div>
            </div>
          </Space>
        )
      }
      extra={
        org && (
          <Space>
            <Button size="small" onClick={() => setEditing((v) => !v)}>
              {editing ? t('doneLabel') : t('edit')}
            </Button>
            <Button size="small" danger icon={<DeleteOutlined />} onClick={del} />
          </Space>
        )
      }
    >
      {!org && <div className="empty">{t('loading')}</div>}
      {org && editing && (
        <OrgForm
          t={t}
          initial={org}
          onCancel={() => setEditing(false)}
          onSubmit={async (patch) => {
            await api.updateOrganization(org.id, patch)
            setEditing(false)
            await refresh()
            await onChanged()
          }}
        />
      )}
      {org && !editing && (
        <>
          <div className="card field-stack">
            <Row label={t('type')} value={<OrgKindBadge kind={org.kind} t={t} />} />
            {org.website && (
              <Row
                label={t('website')}
                value={
                  <a
                    href={org.website.startsWith('http') ? org.website : `https://${org.website}`}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {org.website}
                  </a>
                }
              />
            )}
            {org.domain && <Row label={t('domain')} value={org.domain} />}
            {org.industry && <Row label={t('industry')} value={org.industry} />}
            {org.size && <Row label={t('size')} value={org.size} />}
            {org.address && <Row label={t('address')} value={org.address} />}
            <Row label={t('openDealValue')} value={formatMoney(org.open_deal_value, 'VND')} />
            <Row label={t('updatedAt')} value={fmtDateTime(org.updated_at)} />
            {org.tags.length > 0 && (
              <Row
                label={t('tags')}
                value={
                  <Space size={4} wrap>
                    {org.tags.map((x) => (
                      <Chip key={x} color="#8e8e93">
                        #{x}
                      </Chip>
                    ))}
                  </Space>
                }
              />
            )}
          </div>

          {org.notes && (
            <div className="card">
              <div className="section-title">📝 {t('notes')}</div>
              <div className="notes">{org.notes}</div>
            </div>
          )}

          <div className="card">
            <div className="section-title">
              👥 {t('orgContacts')} ({d!.contacts.length})
            </div>
            {d!.contacts.length === 0 && <div className="empty small">{t('empty')}</div>}
            {d!.contacts.map((c) => (
              <div key={c.customer_id} className="rel-row">
                <Avatar name={c.name} url={c.avatar_url} size={28} />
                <div className="rel-body">
                  <button className="linklike" onClick={() => onPickCustomer(c.customer_id)}>
                    {c.name}
                  </button>
                  {c.is_primary && <Chip color="#5e4ae3">★ {t('primaryOrg')}</Chip>}
                  <div className="task-sub">{c.role_title || c.email || '—'}</div>
                </div>
              </div>
            ))}
          </div>

          <div className="card">
            <div className="section-title">
              💼 {t('orgDeals')} ({d!.deals.length})
            </div>
            {d!.deals.length === 0 && <div className="empty small">{t('noDeals')}</div>}
            {d!.deals.map((deal) => (
              <div key={deal.id} className="deal-row">
                <div className="deal-row-main">
                  <div className="deal-title">{deal.title}</div>
                  <div className="deal-sub">
                    {deal.customer_name} · {formatMoney(deal.amount, deal.currency)}
                  </div>
                </div>
                <DealStageBadge stage={deal.stage} t={t} />
              </div>
            ))}
          </div>
        </>
      )}
    </Drawer>
  )
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="field-stack-row">
      <div className="fs-label">{label}</div>
      <div className="fs-value">{value}</div>
    </div>
  )
}

function OrgFormModal({ t, onClose, onSaved }: { t: T; onClose: () => void; onSaved: () => Promise<void> }) {
  return (
    <Modal open onCancel={onClose} title={t('addOrganization')} footer={null} width={600}>
      <OrgForm
        t={t}
        onCancel={onClose}
        onSubmit={async (input) => {
          await api.createOrganization(input)
          await onSaved()
        }}
      />
    </Modal>
  )
}

function OrgForm({
  t,
  initial,
  onCancel,
  onSubmit,
}: {
  t: T
  initial?: Organization
  onCancel: () => void
  onSubmit: (v: OrganizationInput) => Promise<void>
}) {
  const [form, setForm] = useState<OrganizationInput>({
    name: initial?.name ?? '',
    kind: initial?.kind ?? 'direct_customer',
    website: initial?.website ?? '',
    domain: initial?.domain ?? '',
    industry: initial?.industry ?? '',
    size: initial?.size ?? '',
    address: initial?.address ?? '',
    logo_url: initial?.logo_url ?? '',
    notes: initial?.notes ?? '',
    tags: initial?.tags ?? [],
  })
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  function set<K extends keyof OrganizationInput>(k: K, v: OrganizationInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function save() {
    if (!form.name?.trim()) {
      setErr(t('orgNameRequired'))
      return
    }
    setBusy(true)
    setErr('')
    try {
      await onSubmit({ ...form, name: form.name.trim() })
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <div className="edit-grid">
        <Field label={`${t('name')} *`}>
          <Input autoFocus value={form.name} onChange={(e) => set('name', e.target.value)} />
        </Field>
        <Field label={t('type')}>
          <Select
            value={form.kind}
            onChange={(v) => set('kind', v)}
            options={orgKindOptions(t)}
            style={{ width: '100%' }}
          />
        </Field>
        <Field label={t('website')}>
          <Input value={form.website} onChange={(e) => set('website', e.target.value)} placeholder="https://…" />
        </Field>
        <Field label={t('domain')}>
          <Input value={form.domain} onChange={(e) => set('domain', e.target.value)} placeholder="example.com" />
        </Field>
        <Field label={t('industry')}>
          <Input value={form.industry} onChange={(e) => set('industry', e.target.value)} />
        </Field>
        <Field label={t('size')}>
          <Input value={form.size} onChange={(e) => set('size', e.target.value)} placeholder="1-10, 50+…" />
        </Field>
        <Field label={t('address')} full>
          <Input value={form.address} onChange={(e) => set('address', e.target.value)} />
        </Field>
        <Field label={t('tags')} full>
          <Select
            mode="tags"
            value={form.tags}
            onChange={(v) => set('tags', v)}
            style={{ width: '100%' }}
            tokenSeparators={[',']}
          />
        </Field>
        <Field label={t('notes')} full>
          <Input.TextArea rows={3} value={form.notes} onChange={(e) => set('notes', e.target.value)} />
        </Field>
      </div>
      {err && <div className="err inline">{err}</div>}
      <div className="formactions">
        <Button onClick={onCancel}>{t('cancel')}</Button>
        <Button type="primary" loading={busy} onClick={save}>
          {t('save')}
        </Button>
      </div>
    </div>
  )
}
