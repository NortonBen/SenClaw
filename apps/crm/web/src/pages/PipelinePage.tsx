import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Button, Drawer, Segmented, Select, Space, Switch, Tag, Timeline } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { api, fmtDateTime, type LeadDetail, type SaleState, type Sequence } from '../api'
import { SALE_INTENTS, SALE_STAGES, SALE_STAGE_COLORS, TEMP_META, TEMPERATURES } from '../constants'
import { tk, type T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { Avatar } from '../components/Avatar'
import { Chip, TempBadge, saleStageOptions } from '../components/chips'
import { relTime } from '../components/Field'

export function PipelinePage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [leads, setLeads] = useState<SaleState[]>([])
  const [q, setQ] = useState('')
  const [temp, setTemp] = useState<string | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
  const [dragging, setDragging] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setLeads(await api.listLeads({ q: q || undefined, temperature: temp ?? undefined, limit: 500 }))
    } finally {
      setLoading(false)
    }
  }, [q, temp])

  useEffect(() => {
    const h = setTimeout(refresh, 200)
    return () => clearTimeout(h)
  }, [refresh])

  async function drop(customerId: number, stage: string) {
    setDragging(null)
    await api.setLeadStage(customerId, { stage })
    await refresh()
  }

  return (
    <PageShell
      title={t('salesPipeline')}
      subtitle={`${leads.length} ${t('leadsCount')}`}
      search={q}
      onSearch={setQ}
      searchPlaceholder={t('searchLeads')}
      filters={
        <Select
          allowClear
          value={temp}
          onChange={(v) => setTemp(v ?? null)}
          placeholder={t('allTemps')}
          style={{ width: 150 }}
          options={TEMPERATURES.map((x) => ({
            value: x,
            label: `${TEMP_META[x]?.icon ?? ''} ${tk(t, 'temp', x)}`,
          }))}
        />
      }
      actions={
        <Button icon={<ReloadOutlined />} loading={loading} onClick={refresh}>
          {t('refresh')}
        </Button>
      }
    >
      <div className="pipeline sale-pipeline">
        {SALE_STAGES.map((stage) => {
          const col = leads.filter((l) => l.sale_stage === stage)
          return (
            <div
              key={stage}
              className="kanban-col"
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault()
                if (dragging != null) drop(dragging, stage)
              }}
            >
              <div className="kanban-head" style={{ borderTopColor: SALE_STAGE_COLORS[stage] }}>
                <span>{tk(t, 'saleStage', stage)}</span>
                <span className="kanban-count">{col.length}</span>
              </div>
              <div className="kanban-cards">
                {col.map((l) => (
                  <div
                    key={l.customer_id}
                    className="kanban-card lead-card"
                    draggable
                    onDragStart={() => setDragging(l.customer_id)}
                    onDragEnd={() => setDragging(null)}
                    onClick={() => setOpenId(l.customer_id)}
                  >
                    <div className="lead-card-head">
                      <Avatar name={l.name} size={24} />
                      <span className="kc-title">{l.name}</span>
                    </div>
                    <div className="lead-card-meta">
                      <TempBadge temp={l.temperature} t={t} />
                      <Chip color="#8e8e93">
                        {l.lead_score} {t('score')}
                      </Chip>
                    </div>
                    {l.last_inbound_at && (
                      <div className="muted small">
                        💬 {relTime(l.last_inbound_at)}
                        {l.unsubscribed && ' · 🚫'}
                      </div>
                    )}
                  </div>
                ))}
                {col.length === 0 && <div className="kanban-empty">{t('emptyCol')}</div>}
              </div>
            </div>
          )
        })}
      </div>

      <LeadDrawer
        id={openId}
        t={t}
        onClose={() => setOpenId(null)}
        onChanged={refresh}
        onPickCustomer={onPickCustomer}
      />
    </PageShell>
  )
}

function LeadDrawer({
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
  const [d, setD] = useState<LeadDetail | null>(null)
  const [sequences, setSequences] = useState<Sequence[]>([])
  const [seqKey, setSeqKey] = useState<string | undefined>()
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const { message } = AntApp.useApp()

  const refresh = useCallback(async () => {
    if (id == null) return
    setD(await api.getLead(id))
  }, [id])

  useEffect(() => {
    setDraft('')
    if (id == null) setD(null)
    else {
      refresh()
      api.listSequences().then(setSequences).catch(() => setSequences([]))
    }
  }, [id, refresh])

  if (id == null || !d) return <Drawer open={false} onClose={onClose} />
  const lead = d.lead

  /// One proactive turn: decide, draft, and push it through the guardrail. The
  /// reply may be a send OR a parked review — the toast reports which.
  async function nextAction(intent: string) {
    setBusy(intent)
    try {
      const r = await api.leadNextAction(id!, { intent })
      const status = String((r as Record<string, unknown>).status ?? 'ok')
      message.info(`${tk(t, 'intent', intent)} → ${status}`)
      await refresh()
      await onChanged()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(null)
    }
  }

  async function preview(intent: string) {
    setBusy('draft')
    try {
      const r = await api.leadDraft(id!, { intent })
      setDraft(r.draft)
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(null)
    }
  }

  async function startSeq() {
    if (!seqKey) return
    setBusy('seq')
    try {
      await api.startLeadSequence(id!, seqKey)
      message.success(t('sequenceStarted'))
      await refresh()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <Drawer
      open
      onClose={onClose}
      width={620}
      title={
        <Space>
          <Avatar name={lead.name} url={d.customer?.avatar_url} size={36} />
          <div>
            <div style={{ fontWeight: 600 }}>{lead.name}</div>
            <div className="muted small">{d.customer?.company || t('leadDrawer')}</div>
          </div>
        </Space>
      }
      extra={
        <Button size="small" onClick={() => onPickCustomer(lead.customer_id)}>
          {t('openProfile')}
        </Button>
      }
    >
      {/* stage + temperature */}
      <div className="card">
        <div className="section-title">📈 {t('salesStage')}</div>
        <Segmented
          block
          value={lead.sale_stage}
          options={saleStageOptions(t)}
          onChange={async (v) => {
            setD({ ...d, lead: await api.setLeadStage(id!, { stage: String(v) }) })
            await onChanged()
          }}
        />
        <Space wrap style={{ marginTop: 10 }}>
          <Select
            size="small"
            value={lead.temperature}
            style={{ width: 130 }}
            options={TEMPERATURES.map((x) => ({
              value: x,
              label: `${TEMP_META[x]?.icon ?? ''} ${tk(t, 'temp', x)}`,
            }))}
            onChange={async (v) => {
              setD({ ...d, lead: await api.setLeadStage(id!, { temperature: v }) })
              await onChanged()
            }}
          />
          <Chip color="#8e8e93">
            {lead.lead_score} {t('score')}
          </Chip>
          <span className="muted small">
            {t('checkins')}: {lead.checkin_count}
          </span>
          <span className="muted small">
            {t('lastInbound')}: {relTime(lead.last_inbound_at)}
          </span>
        </Space>
        <div style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 8 }}>
          <Switch
            size="small"
            checked={lead.unsubscribed}
            onChange={async (v) => {
              setD({ ...d, lead: await api.setLeadUnsubscribed(id!, v) })
              await onChanged()
            }}
          />
          <span className={lead.unsubscribed ? 'warn' : 'muted'}>🚫 {t('unsubscribed')}</span>
        </div>
        {lead.intent_signals.length > 0 && (
          <div style={{ marginTop: 8 }}>
            <span className="muted small">{t('intentSignals')}: </span>
            {lead.intent_signals.map((s) => (
              <Tag key={s}>{s}</Tag>
            ))}
          </div>
        )}
      </div>

      {/* proactive intents */}
      <div className="card">
        <div className="section-title">⚡ {t('proactive')}</div>
        <Space wrap>
          {SALE_INTENTS.map((intent) => (
            <Button
              key={intent}
              size="small"
              type="primary"
              loading={busy === intent}
              disabled={lead.unsubscribed}
              onClick={() => nextAction(intent)}
            >
              {tk(t, 'intent', intent)}
            </Button>
          ))}
        </Space>
        <Space wrap style={{ marginTop: 8 }}>
          {SALE_INTENTS.map((intent) => (
            <Button key={intent} size="small" loading={busy === 'draft'} onClick={() => preview(intent)}>
              👁 {tk(t, 'intent', intent)}
            </Button>
          ))}
        </Space>
        {draft && (
          <div className="ai-out" style={{ marginTop: 10 }}>
            <div className="muted small">{t('draftPreview')}</div>
            {draft}
          </div>
        )}
      </div>

      {/* sequences */}
      <div className="card">
        <div className="section-title">🔁 {t('sequences')}</div>
        <Space wrap>
          <Select
            size="small"
            style={{ minWidth: 200 }}
            value={seqKey}
            onChange={setSeqKey}
            placeholder={t('sequences')}
            options={sequences.filter((s) => s.enabled).map((s) => ({ value: s.key, label: s.name }))}
          />
          <Button size="small" type="primary" loading={busy === 'seq'} disabled={!seqKey} onClick={startSeq}>
            {t('startSequence')}
          </Button>
        </Space>
        {d.runs.length > 0 && (
          <div style={{ marginTop: 8 }}>
            {d.runs.map((r) => (
              <div key={r.id} className="muted small">
                {r.sequence_key} · step {r.current_step} · <Tag>{r.status}</Tag>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* transcript */}
      <div className="card">
        <div className="section-title">💬 {t('transcript')}</div>
        {d.messages.length === 0 && <div className="empty small">{t('noMessages')}</div>}
        <div className="mini-thread">
          {d.messages.map((m) => (
            <div key={m.id} className={'bubble-row ' + (m.direction === 'inbound' ? 'in' : 'out')}>
              <div className={'bubble ' + (m.direction === 'inbound' ? 'in' : 'out')}>
                <div className="bubble-text">{m.content}</div>
                <div className="bubble-meta">
                  {m.direction === 'inbound' ? t('customerSide') : t('me')} · {fmtDateTime(m.created_at)}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* agent reasoning */}
      <div className="card">
        <div className="section-title">🧠 {t('agentLog')}</div>
        {d.actions.length === 0 ? (
          <div className="empty small">{t('noAgentLog')}</div>
        ) : (
          <Timeline
            items={d.actions.map((a) => ({
              color: a.needs_review ? 'orange' : 'blue',
              children: (
                <div>
                  <div>
                    <b>{a.action_type}</b>{' '}
                    {a.needs_review && <Chip color="#ff9500">{t('pendingReviews')}</Chip>}
                    <span className="muted small"> · {fmtDateTime(a.created_at)}</span>
                  </div>
                  {a.reasoning && <div className="muted small">{a.reasoning}</div>}
                </div>
              ),
            }))}
          />
        )}
      </div>
    </Drawer>
  )
}
