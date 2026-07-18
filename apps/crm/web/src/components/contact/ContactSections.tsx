import { useCallback, useEffect, useMemo, useState } from 'react'
import { Button, Card, InputNumber, Select, Space, Tag } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import {
  api,
  fmtDateTime,
  type Customer,
  type CustomerChannel,
  type Interaction,
  type OrgMembership,
  type Organization,
  type Relationship,
  type SaleState,
} from '../../api'
import { CHANNEL_KINDS, CHANNEL_META, KIND_ICONS, KIND_ORDER, REL_ORDER, channelMeta } from '../../constants'
import { fmt, tk, type T } from '../../i18n'
import { Chip, SaleStageBadge, TempBadge, saleStageOptions, tempOptions } from '../chips'

// ---------- AI briefing ----------

export function AISection({ customerId, t }: { customerId: number; t: T }) {
  const [summary, setSummary] = useState<{ text: string; model: string } | null>(null)
  const [next, setNext] = useState<{ text: string; model: string } | null>(null)
  const [busy, setBusy] = useState<'sum' | 'next' | null>(null)
  const [err, setErr] = useState('')

  async function runSum() {
    setBusy('sum')
    setErr('')
    try {
      setSummary(await api.summarize(customerId))
    } catch (e) {
      setErr(t('needLlm') + String(e))
    } finally {
      setBusy(null)
    }
  }
  async function runNext() {
    setBusy('next')
    setErr('')
    try {
      setNext(await api.nextStep(customerId))
    } catch (e) {
      setErr(t('needLlm') + String(e))
    } finally {
      setBusy(null)
    }
  }
  return (
    <div className="card ai">
      <div className="section-title">✨ {t('aiBriefing')}</div>
      <Space wrap>
        <Button type="primary" size="small" loading={busy === 'sum'} onClick={runSum}>
          {busy === 'sum' ? t('aiSummarizing') : t('aiSummarize')}
        </Button>
        <Button size="small" loading={busy === 'next'} onClick={runNext}>
          {busy === 'next' ? t('aiNextStepBusy') : t('aiNextStep')}
        </Button>
      </Space>
      {summary && <div className="ai-out">{summary.text}</div>}
      {next && <div className="ai-out next">👉 {next.text}</div>}
      {err && <div className="err inline">{err}</div>}
    </div>
  )
}

// ---------- contact channels (their identities) ----------

export function ChannelsSection({ customerId, t }: { customerId: number; t: T }) {
  const [channels, setChannels] = useState<CustomerChannel[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setChannels(await api.listChannels(customerId))
  }, [customerId])
  useEffect(() => {
    refresh()
  }, [refresh])

  async function del(id: number) {
    if (!confirm(t('deleteChannel'))) return
    await api.deleteChannel(id)
    await refresh()
  }

  return (
    <Card
      className="channels-card"
      style={{ margin: '14px 0' }}
      title={
        <span className="card-title">
          📞 {t('channels')} ({channels.length})
        </span>
      }
      extra={
        <Button size="small" icon={<PlusOutlined />} onClick={() => setShowAdd(true)}>
          {t('addChannel')}
        </Button>
      }
    >
      {channels.length === 0 && !showAdd && <div className="empty small">{t('noChannels')}</div>}
      <div className="channel-list">
        {channels.map((ch) => {
          const meta = channelMeta(ch.kind)
          if (editingId === ch.id) {
            return (
              <ChannelForm
                key={ch.id}
                initial={ch}
                t={t}
                onCancel={() => setEditingId(null)}
                onSave={async (patch) => {
                  await api.updateChannel(ch.id, patch)
                  setEditingId(null)
                  await refresh()
                }}
              />
            )
          }
          return (
            <div key={ch.id} className="channel-row" style={{ borderLeft: `3px solid ${meta.color}` }}>
              <span className="channel-icon" style={{ background: meta.color + '22', color: meta.color }}>
                {meta.icon}
              </span>
              <div className="channel-body">
                <div className="channel-line1">
                  <a
                    href={meta.href(ch.value)}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: meta.color, fontWeight: 500 }}
                  >
                    {ch.value}
                  </a>
                  {ch.label && <Tag>{ch.label}</Tag>}
                </div>
                <div className="channel-line2 muted small">{tk(t, 'chKind', ch.kind)}</div>
              </div>
              <Button size="small" type="text" onClick={() => setEditingId(ch.id)}>
                ✎
              </Button>
              <Button size="small" type="text" danger onClick={() => del(ch.id)}>
                ×
              </Button>
            </div>
          )
        })}
      </div>
      {showAdd && (
        <ChannelForm
          t={t}
          onCancel={() => setShowAdd(false)}
          onSave={async (v) => {
            await api.addChannel(customerId, { kind: v.kind!, value: v.value!, label: v.label })
            setShowAdd(false)
            await refresh()
          }}
        />
      )}
    </Card>
  )
}

export function ChannelForm({
  initial,
  t,
  onCancel,
  onSave,
}: {
  initial?: CustomerChannel
  t: T
  onCancel: () => void
  onSave: (v: { kind?: string; value?: string; label?: string }) => Promise<void>
}) {
  const [kind, setKind] = useState<string>(initial?.kind ?? 'zalo')
  const [value, setValue] = useState<string>(initial?.value ?? '')
  const [label, setLabel] = useState<string>(initial?.label ?? '')
  const [busy, setBusy] = useState(false)
  const meta = channelMeta(kind)

  async function save() {
    if (!value.trim()) return
    setBusy(true)
    try {
      await onSave({ kind, value: value.trim(), label: label.trim() })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="channel-form">
      <Select
        value={kind}
        onChange={setKind}
        style={{ minWidth: 170 }}
        options={CHANNEL_KINDS.map((k) => ({
          value: k,
          label: (
            <span>
              {CHANNEL_META[k]!.icon} {tk(t, 'chKind', k)}
            </span>
          ),
        }))}
      />
      <input
        className="plain-input"
        value={value}
        placeholder={meta.placeholder}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && save()}
      />
      <input
        className="plain-input"
        value={label}
        placeholder={t('channelLabelPh')}
        onChange={(e) => setLabel(e.target.value)}
        style={{ maxWidth: 200 }}
      />
      <Button size="small" onClick={onCancel}>
        {t('cancel')}
      </Button>
      <Button size="small" type="primary" loading={busy} disabled={!value.trim()} onClick={save}>
        {t('save')}
      </Button>
    </div>
  )
}

// ---------- organizations multi-select ----------

/// `POST /api/customers/:id/organizations` takes `{organization_id}` OR
/// `{organization_name}` — the latter resolves-or-creates server-side, which is
/// what lets this be a plain tags-mode select with no "create org" round trip.
export function OrganizationsSection({ customerId, t }: { customerId: number; t: T }) {
  const [orgs, setOrgs] = useState<OrgMembership[]>([])
  const [all, setAll] = useState<Organization[]>([])
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    setOrgs(await api.customerOrgs(customerId))
  }, [customerId])
  useEffect(() => {
    refresh()
    api.listOrganizations({ limit: 500 }).then(setAll)
  }, [refresh])

  const linkedIds = useMemo(() => new Set(orgs.map((o) => o.organization_id)), [orgs])

  async function onSelect(value: unknown) {
    if (typeof value !== 'string' || !value.trim()) return
    setBusy(true)
    try {
      const existing = all.find((o) => String(o.id) === value || o.name.toLowerCase() === value.toLowerCase())
      setOrgs(
        await api.linkCustomerOrg(customerId, existing
          ? { organization_id: existing.id, is_primary: orgs.length === 0 }
          : { organization_name: value, is_primary: orgs.length === 0 }),
      )
      api.listOrganizations({ limit: 500 }).then(setAll)
    } finally {
      setBusy(false)
    }
  }

  async function unlink(orgId: number) {
    if (!confirm(t('unlinkOrg'))) return
    setOrgs(await api.unlinkCustomerOrg(customerId, orgId))
  }

  async function setPrimary(o: OrgMembership) {
    setOrgs(
      await api.linkCustomerOrg(customerId, {
        organization_id: o.organization_id,
        role_title: o.role_title,
        is_primary: true,
      }),
    )
  }

  return (
    <div className="field-stack-row">
      <div className="fs-label">🏢 {t('organizations')}</div>
      <div className="fs-value">
        <div className="org-chips">
          {orgs.map((o) => (
            <span key={o.organization_id} className="org-chip">
              <Chip color={o.is_primary ? '#5e4ae3' : '#8e8e93'} title={o.role_title}>
                {o.name}
                {o.is_primary && ' ★'}
              </Chip>
              {!o.is_primary && (
                <button className="org-chip-btn" title={t('setPrimary')} onClick={() => setPrimary(o)}>
                  ★
                </button>
              )}
              <button className="org-chip-btn" title={t('del')} onClick={() => unlink(o.organization_id)}>
                ×
              </button>
            </span>
          ))}
          {orgs.length === 0 && <span className="muted small">{t('noOrgsLinked')}</span>}
        </div>
        <Select
          showSearch
          allowClear
          value={null}
          loading={busy}
          placeholder={t('orgPickerPh')}
          style={{ minWidth: 220, marginTop: 6 }}
          optionFilterProp="label"
          onSelect={onSelect}
          // tags mode so a name that isn't in the list is still selectable —
          // the server resolves-or-creates it.
          mode="tags"
          maxCount={1}
          options={all
            .filter((o) => !linkedIds.has(o.id))
            .map((o) => ({ value: String(o.id), label: o.name }))}
        />
      </div>
    </div>
  )
}

// ---------- sales stage / temperature ----------

export function SalesStateSection({
  customerId,
  t,
  onChanged,
}: {
  customerId: number
  t: T
  onChanged?: () => void
}) {
  const [lead, setLead] = useState<SaleState | null>(null)

  const refresh = useCallback(async () => {
    try {
      setLead((await api.getLead(customerId)).lead)
    } catch {
      setLead(null)
    }
  }, [customerId])
  useEffect(() => {
    refresh()
  }, [refresh])

  if (!lead) return null

  async function patch(body: { stage?: string; temperature?: string; lead_score?: number }) {
    setLead(await api.setLeadStage(customerId, body))
    onChanged?.()
  }

  return (
    <>
      <div className="field-stack-row">
        <div className="fs-label">📈 {t('salesStage')}</div>
        <div className="fs-value">
          <Space wrap>
            <Select
              size="small"
              value={lead.sale_stage}
              style={{ minWidth: 160 }}
              options={saleStageOptions(t)}
              onChange={(v) => patch({ stage: v })}
            />
            <Select
              size="small"
              value={lead.temperature}
              style={{ minWidth: 120 }}
              options={tempOptions(t)}
              onChange={(v) => patch({ temperature: v })}
            />
            {/* No `addonAfter` — the addon overflowed the narrow left column.
                The unit rides alongside as plain text instead. */}
            <InputNumber
              size="small"
              min={0}
              max={100}
              value={lead.lead_score}
              onChange={(v) => patch({ lead_score: Number(v) || 0 })}
              suffix={<span className="muted small">{t('score')}</span>}
              style={{ width: 104 }}
            />
          </Space>
          {lead.unsubscribed && (
            <div style={{ marginTop: 6 }}>
              <Chip color="#ff3b30">🚫 {t('unsubscribed')}</Chip>
            </div>
          )}
        </div>
      </div>
      <div className="field-stack-row">
        <div className="fs-label">👤 {t('owner')}</div>
        <div className="fs-value">
          {lead.owner ? <Chip color="#007aff">{lead.owner}</Chip> : <span className="muted small">—</span>}
          {lead.intent_signals.length > 0 && (
            <div style={{ marginTop: 6 }}>
              {lead.intent_signals.map((s) => (
                <Tag key={s}>{s}</Tag>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  )
}

/// Read-only sales row for the detail header — badges only.
export function SalesBadges({ lead, t }: { lead: SaleState | null; t: T }) {
  if (!lead) return null
  return (
    <Space size={4} wrap>
      <SaleStageBadge stage={lead.sale_stage} t={t} />
      <TempBadge temp={lead.temperature} t={t} />
      <Chip color="#8e8e93">
        {lead.lead_score} {t('score')}
      </Chip>
    </Space>
  )
}

// ---------- relationships ----------

export function RelationshipsSection({
  customer,
  t,
  onPickCustomer,
}: {
  customer: Customer
  t: T
  onPickCustomer?: (id: number) => void
}) {
  const [rels, setRels] = useState<Relationship[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [extract, setExtract] = useState<{ busy: boolean; result: string; err: string }>({
    busy: false,
    result: '',
    err: '',
  })

  const refresh = useCallback(async () => {
    setRels(await api.customerRelationships(customer.id))
  }, [customer.id])
  useEffect(() => {
    refresh()
  }, [refresh])

  async function del(id: number) {
    if (!confirm(t('deleteRelationship'))) return
    await api.deleteRelationship(id)
    await refresh()
  }

  async function runExtract() {
    setExtract({ busy: true, result: '', err: '' })
    try {
      const r = await api.extract(customer.id)
      setExtract({
        busy: false,
        result: fmt(t('extractedSummary'), {
          a: r.extracted,
          b: r.mentions_saved,
          c: r.relationships_created,
        }),
        err: '',
      })
      await refresh()
    } catch (e) {
      setExtract({ busy: false, result: '', err: t('needLlm') + String(e) })
    }
  }

  return (
    <div className="card">
      <div className="section-title">
        🕸 {t('relationships')} ({rels.length})
        <span className="right">
          <Button size="small" type="text" loading={extract.busy} onClick={runExtract}>
            ✨ {extract.busy ? t('aiExtracting') : t('aiExtract')}
          </Button>
          <Button size="small" type="text" icon={<PlusOutlined />} onClick={() => setShowAdd(true)}>
            {t('add')}
          </Button>
        </span>
      </div>
      {rels.length === 0 && !showAdd && <div className="empty small">{t('noRelationships')}</div>}
      {rels.map((r) => {
        const isFrom = r.from_id === customer.id
        const otherId = isFrom ? r.to_id : r.from_id
        const otherName = isFrom ? r.to_name : r.from_name
        // Reading direction from THIS customer's perspective.
        const verb = isFrom ? tk(t, 'rel', r.kind) : `${tk(t, 'rel', r.kind)} ${t('inverse')}`
        return (
          <div key={r.id} className="rel-row">
            <div className="rel-dot">🔗</div>
            <div className="rel-body">
              <div>
                <span className="rel-verb">{verb}</span>{' '}
                <button className="linklike" onClick={() => onPickCustomer?.(otherId)} title={`id=${otherId}`}>
                  {otherName}
                </button>
                {r.source === 'ai' && <span className="rel-ai">✨ AI</span>}
              </div>
              {r.note && <div className="task-sub">{r.note}</div>}
            </div>
            <button className="tl-del" onClick={() => del(r.id)} title={t('del')}>
              ×
            </button>
          </div>
        )
      })}
      {showAdd && (
        <AddRelationshipForm
          fromId={customer.id}
          fromName={customer.name}
          t={t}
          onClose={() => setShowAdd(false)}
          onCreated={async () => {
            setShowAdd(false)
            await refresh()
          }}
        />
      )}
      {extract.result && <div className="ai-out">{extract.result}</div>}
      {extract.err && <div className="err inline">{extract.err}</div>}
    </div>
  )
}

function AddRelationshipForm({
  fromId,
  fromName,
  t,
  onClose,
  onCreated,
}: {
  fromId: number
  fromName: string
  t: T
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [customers, setCustomers] = useState<Customer[]>([])
  const [toId, setToId] = useState<number | undefined>()
  const [kind, setKind] = useState<string>('contact_of')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.listCustomers({ limit: 500 }).then((all) => setCustomers(all.filter((c) => c.id !== fromId)))
  }, [fromId])

  async function save() {
    if (toId == null || !kind) return
    setBusy(true)
    try {
      await api.createRelationship({ from_id: fromId, to_id: toId, kind, note: note.trim() || undefined })
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="add-rel">
      <div className="add-rel-line">
        <b>{fromName}</b>
        <Select
          size="small"
          value={kind}
          onChange={setKind}
          style={{ minWidth: 190 }}
          options={REL_ORDER.map((k) => ({ value: k, label: tk(t, 'rel', k) }))}
        />
        <Select
          size="small"
          showSearch
          value={toId}
          onChange={setToId}
          style={{ minWidth: 220 }}
          placeholder={t('pickCustomerDash')}
          optionFilterProp="label"
          options={customers.map((c) => ({
            value: c.id,
            label: c.company ? `${c.name} · ${c.company}` : c.name,
          }))}
        />
      </div>
      <input
        className="plain-input"
        placeholder={t('relNotePh')}
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
      <div className="formactions">
        <Button size="small" onClick={onClose}>
          {t('cancel')}
        </Button>
        <Button size="small" type="primary" loading={busy} disabled={toId == null} onClick={save}>
          {t('addRelationship')}
        </Button>
      </div>
    </div>
  )
}

// ---------- interactions timeline ----------

export function InteractionsSection({
  customerId,
  interactions,
  t,
  onChanged,
}: {
  customerId: number
  interactions: Interaction[]
  t: T
  onChanged: () => Promise<void>
}) {
  const [kind, setKind] = useState<string>('note')
  const [summary, setSummary] = useState('')
  const [details, setDetails] = useState('')
  const [busy, setBusy] = useState(false)

  async function add() {
    if (!summary.trim()) return
    setBusy(true)
    try {
      await api.addInteraction(customerId, {
        kind,
        summary: summary.trim(),
        details: details.trim() || undefined,
      })
      setSummary('')
      setDetails('')
      await onChanged()
    } finally {
      setBusy(false)
    }
  }

  async function del(id: number) {
    if (!confirm(t('deleteInteraction'))) return
    await api.deleteInteraction(id)
    await onChanged()
  }

  return (
    <div className="card">
      <div className="section-title">🕐 {t('history')}</div>
      <div className="new-interaction">
        <Select
          value={kind}
          onChange={setKind}
          style={{ minWidth: 150 }}
          options={KIND_ORDER.map((k) => ({
            value: k,
            label: `${KIND_ICONS[k] ?? '•'} ${tk(t, 'kind', k)}`,
          }))}
        />
        <input
          className="plain-input"
          placeholder={t('addInteractionPh')}
          value={summary}
          onChange={(e) => setSummary(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && add()}
        />
        <Button type="primary" loading={busy} disabled={!summary.trim()} onClick={add}>
          {busy ? t('logging') : t('logIt')}
        </Button>
      </div>
      <textarea
        rows={2}
        placeholder={t('detailsPh')}
        value={details}
        onChange={(e) => setDetails(e.target.value)}
      />
      <div className="timeline">
        {interactions.length === 0 && <div className="empty">{t('noHistory')}</div>}
        {interactions.map((i) => (
          <div className="tl-item" key={i.id}>
            <div className="tl-dot" title={tk(t, 'kind', i.kind)}>
              {KIND_ICONS[i.kind] ?? '•'}
            </div>
            <div className="tl-body">
              <div className="tl-head">
                <span className="tl-summary">{i.summary}</span>
                <span className="tl-when">{fmtDateTime(i.occurred_at)}</span>
                <button className="tl-del" onClick={() => del(i.id)} title={t('del')}>
                  ×
                </button>
              </div>
              {i.details && <div className="tl-details">{i.details}</div>}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

/// Notes box for the detail's right column — inline-saves on blur.
export function NotesBox({
  value,
  t,
  onSave,
}: {
  value: string
  t: T
  onSave: (v: string) => Promise<void>
}) {
  const [text, setText] = useState(value)
  const [dirty, setDirty] = useState(false)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    setText(value)
    setDirty(false)
  }, [value])

  async function save() {
    if (!dirty) return
    setBusy(true)
    try {
      await onSave(text)
      setDirty(false)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="card">
      <div className="section-title">
        📝 {t('notes')}
        {dirty && (
          <Button size="small" type="primary" className="right" loading={busy} onClick={save}>
            {t('save')}
          </Button>
        )}
      </div>
      <textarea
        rows={6}
        value={text}
        placeholder={t('notesPh')}
        onChange={(e) => {
          setText(e.target.value)
          setDirty(true)
        }}
        onBlur={save}
      />
    </div>
  )
}
