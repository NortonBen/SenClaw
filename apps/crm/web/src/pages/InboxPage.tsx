import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { App as AntApp, Button, Input, Modal, Select, Space, Switch, Tag } from 'antd'
import { ApiOutlined, DeleteOutlined, PlusOutlined, ReloadOutlined, SendOutlined } from '@ant-design/icons'
import {
  api,
  fmtDateTime,
  type Conversation,
  type ConversationDetail,
  type Customer,
  type InboxChannel,
} from '../api'
import {
  CHANNEL_CONFIG_FIELDS,
  INBOX_CHANNEL_KINDS,
  SECRET_MASK,
  inboxChannelMeta,
  isSecretField,
} from '../constants'
import { tk, type T } from '../i18n'
import { subscribeEvents } from '../events'
import { PageShell } from '../components/PageShell'
import { Avatar } from '../components/Avatar'
import { Chip } from '../components/chips'
import { Field, relTime } from '../components/Field'

export function InboxPage({
  t,
  customers,
  onPickCustomer,
}: {
  t: T
  customers: Customer[]
  onPickCustomer: (id: number) => void
}) {
  const [convs, setConvs] = useState<Conversation[]>([])
  const [openId, setOpenId] = useState<number | null>(null)
  const [detail, setDetail] = useState<ConversationDetail | null>(null)
  const [q, setQ] = useState('')
  const [showChannels, setShowChannels] = useState(false)
  const [loading, setLoading] = useState(false)

  // A transient failure must leave the last good list on screen rather than
  // blanking the pane — these run on every server event, so one bad fetch
  // would otherwise wipe the inbox.
  const refreshList = useCallback(async () => {
    setLoading(true)
    try {
      const rows = await api.listConversations({ q: q || undefined, limit: 200 })
      setConvs(rows)
      setOpenId((cur) => cur ?? rows[0]?.id ?? null)
    } catch {
      /* keep what we have */
    } finally {
      setLoading(false)
    }
  }, [q])

  const refreshDetail = useCallback(async () => {
    if (openId == null) {
      setDetail(null)
      return
    }
    try {
      const d = await api.getConversation(openId)
      setDetail(d)
      if (d.conversation.unread > 0) {
        await api.markConversationRead(openId).catch(() => {})
      }
    } catch {
      /* keep the thread we have */
    }
  }, [openId])

  useEffect(() => {
    const h = setTimeout(refreshList, 200)
    return () => clearTimeout(h)
  }, [refreshList])

  useEffect(() => {
    refreshDetail()
  }, [refreshDetail])

  // Live refresh. The SSE payload is only a nudge — we refetch rather than
  // patch state from the event, which is exactly how the sibling apps do it.
  useEffect(
    () =>
      subscribeEvents(() => {
        refreshList()
        refreshDetail()
      }),
    [refreshList, refreshDetail],
  )

  return (
    <PageShell
      title={t('navInbox')}
      subtitle={`${convs.length} ${t('conversations').toLowerCase()}`}
      search={q}
      onSearch={setQ}
      searchPlaceholder={t('search')}
      actions={
        <Space>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={refreshList}>
            {t('refresh')}
          </Button>
          <Button icon={<ApiOutlined />} onClick={() => setShowChannels(true)}>
            {t('channelAccounts')}
          </Button>
        </Space>
      }
      bare
    >
      <div className="inbox">
        <div className="inbox-list">
          {convs.length === 0 && <div className="empty">{t('noConversations')}</div>}
          {convs.map((c) => (
            <ConvRow key={c.id} c={c} t={t} selected={c.id === openId} onPick={() => setOpenId(c.id)} />
          ))}
        </div>
        <div className="inbox-thread">
          {detail ? (
            <Thread
              d={detail}
              t={t}
              customers={customers}
              onChanged={async () => {
                await refreshDetail()
                await refreshList()
              }}
              onPickCustomer={onPickCustomer}
            />
          ) : (
            <div className="empty big">{t('pickConversation')}</div>
          )}
        </div>
      </div>

      {showChannels && <ChannelsModal t={t} onClose={() => setShowChannels(false)} />}
    </PageShell>
  )
}

function ConvRow({
  c,
  t,
  selected,
  onPick,
}: {
  c: Conversation
  t: T
  selected: boolean
  onPick: () => void
}) {
  const meta = inboxChannelMeta(c.channel_kind)
  const name = c.customer_name || c.display_name || c.external_id
  return (
    <div className={'conv-row' + (selected ? ' sel' : '')} onClick={onPick}>
      <Avatar name={name} url={c.customer_avatar || undefined} size={38} />
      <div className="conv-body">
        <div className="conv-line1">
          <span className="conv-name">{name}</span>
          <span className="conv-chan" title={tk(t, 'chan', c.channel_kind)} style={{ color: meta.color }}>
            {meta.icon}
          </span>
          <span className="conv-when">{relTime(c.last_message_at ?? c.created_at)}</span>
        </div>
        <div className="conv-line2">
          <span className="conv-preview">{c.preview || '—'}</span>
          {c.unread > 0 && <span className="conv-unread" />}
        </div>
        {c.customer_id === 0 && (
          <div className="conv-line3">
            <Chip color="#ff9500">{t('unlinkedThread')}</Chip>
          </div>
        )}
      </div>
    </div>
  )
}

function Thread({
  d,
  t,
  customers,
  onChanged,
  onPickCustomer,
}: {
  d: ConversationDetail
  t: T
  customers: Customer[]
  onChanged: () => Promise<void>
  onPickCustomer: (id: number) => void
}) {
  const c = d.conversation
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [linkTo, setLinkTo] = useState<number | undefined>()
  const endRef = useRef<HTMLDivElement>(null)
  const { message } = AntApp.useApp()
  const meta = inboxChannelMeta(c.channel_kind)

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: 'end' })
  }, [d.messages.length, c.id])

  async function send() {
    const body = text.trim()
    if (!body) return
    setBusy(true)
    try {
      await api.sendConvMessage(c.id, { text: body, by: 'operator' })
      setText('')
      await onChanged()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(false)
    }
  }

  async function link() {
    if (linkTo == null) return
    await api.linkConversation(c.id, linkTo)
    setLinkTo(undefined)
    await onChanged()
  }

  const handoffLabel =
    c.handoff_state === 'bot' ? t('handoffBot') : c.handoff_state === 'pending' ? t('handoffPending') : t('handoffOperator')

  return (
    <div className="thread">
      <div className="thread-head">
        <Avatar name={c.customer_name || c.display_name || c.external_id} url={c.customer_avatar || undefined} size={34} />
        <div className="thread-head-main">
          <div className="thread-name">
            {c.customer_id !== 0 ? (
              <button className="linklike" onClick={() => onPickCustomer(c.customer_id)}>
                {c.customer_name || c.display_name}
              </button>
            ) : (
              c.display_name || c.external_id
            )}
          </div>
          <div className="muted small">
            <span style={{ color: meta.color }}>{meta.icon}</span> {tk(t, 'chan', c.channel_kind)} ·{' '}
            {c.external_id} · {c.message_count} msg
          </div>
        </div>
        <Space>
          <Chip color={c.handoff_state === 'bot' ? '#5e4ae3' : '#ff9500'}>{handoffLabel}</Chip>
          <Select
            size="small"
            value={c.status}
            style={{ width: 110 }}
            onChange={async (v) => {
              await api.setConversationStatus(c.id, v)
              await onChanged()
            }}
            options={[
              { value: 'open', label: t('statusOpen') },
              { value: 'snoozed', label: t('statusSnoozed') },
              { value: 'closed', label: t('statusClosed') },
            ]}
          />
        </Space>
      </div>

      {/* An unlinked thread (customer_id === 0) is one nobody has claimed — the
          picker is the only way to attach it to a contact. */}
      {c.customer_id === 0 && (
        <div className="thread-link-bar">
          <span className="muted small">{t('linkToContact')}:</span>
          <Select
            showSearch
            size="small"
            style={{ minWidth: 240 }}
            placeholder={t('linkPickerPh')}
            value={linkTo}
            onChange={setLinkTo}
            optionFilterProp="label"
            options={customers.map((x) => ({
              value: x.id,
              label: x.company ? `${x.name} · ${x.company}` : x.name,
            }))}
          />
          <Button size="small" type="primary" disabled={linkTo == null} onClick={link}>
            {t('linkToContact')}
          </Button>
        </div>
      )}

      <div className="thread-body">
        {d.messages.length === 0 && <div className="empty">{t('noMessages')}</div>}
        {d.messages.map((m) => (
          <div key={m.id} className={'bubble-row ' + (m.direction === 'inbound' ? 'in' : 'out')}>
            <div className={'bubble ' + (m.direction === 'inbound' ? 'in' : 'out') + ' role-' + m.role}>
              <div className="bubble-text">{m.content}</div>
              <div className="bubble-meta">
                {m.role} · {fmtDateTime(m.created_at)}
                {m.status && m.status !== 'sent' && ` · ${m.status}`}
              </div>
            </div>
          </div>
        ))}
        <div ref={endRef} />
      </div>

      <div className="thread-composer">
        <Input.TextArea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={t('composerPh')}
          autoSize={{ minRows: 1, maxRows: 5 }}
          onPressEnter={(e) => {
            if (!e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
        />
        <Button type="primary" icon={<SendOutlined />} loading={busy} disabled={!text.trim()} onClick={send}>
          {busy ? t('sending') : t('send')}
        </Button>
      </div>
    </div>
  )
}

// ---------- connected channel accounts ----------

function ChannelsModal({ t, onClose }: { t: T; onClose: () => void }) {
  const [channels, setChannels] = useState<InboxChannel[]>([])
  const [editing, setEditing] = useState<InboxChannel | null>(null)
  const [creating, setCreating] = useState(false)
  const { message } = AntApp.useApp()

  const refresh = useCallback(async () => {
    setChannels(await api.listInboxChannels())
  }, [])
  useEffect(() => {
    refresh()
  }, [refresh])

  async function del(id: number) {
    if (!confirm(t('deleteChannelAccount'))) return
    await api.deleteInboxChannel(id)
    await refresh()
  }

  async function test(id: number) {
    const r = await api.testInboxChannel(id)
    if (r.ok) message.success(t('testOk'))
    else message.error(`${t('testFail')}: ${r.error ?? ''}`)
    await refresh()
  }

  return (
    <Modal open onCancel={onClose} title={t('channelAccounts')} width={720} footer={null}>
      {channels.length === 0 && !creating && <div className="empty small">{t('noChannelAccounts')}</div>}
      {channels.map((ch) => {
        const meta = inboxChannelMeta(ch.kind)
        if (editing?.id === ch.id) {
          return (
            <ChannelAccountForm
              key={ch.id}
              t={t}
              initial={ch}
              onCancel={() => setEditing(null)}
              onSaved={async () => {
                setEditing(null)
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
                <b>{ch.name || tk(t, 'chan', ch.kind)}</b>
                <Tag color={ch.enabled ? 'green' : 'default'}>{ch.enabled ? t('active') : t('inactive')}</Tag>
                {ch.last_status && <Tag>{ch.last_status}</Tag>}
              </div>
              <div className="channel-line2 muted small">
                {tk(t, 'chan', ch.kind)}
                {ch.last_sync_at ? ` · ${t('lastSync')}: ${fmtDateTime(ch.last_sync_at)}` : ''}
                {ch.last_error ? ` · ⚠ ${ch.last_error}` : ''}
              </div>
            </div>
            <Space>
              <Switch
                size="small"
                checked={ch.enabled}
                onChange={async (v) => {
                  await api.updateInboxChannel(ch.id, { enabled: v })
                  await refresh()
                }}
              />
              <Button size="small" onClick={() => test(ch.id)}>
                {t('testChannel')}
              </Button>
              <Button size="small" type="text" onClick={() => setEditing(ch)}>
                ✎
              </Button>
              <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={() => del(ch.id)} />
            </Space>
          </div>
        )
      })}

      {creating ? (
        <ChannelAccountForm
          t={t}
          onCancel={() => setCreating(false)}
          onSaved={async () => {
            setCreating(false)
            await refresh()
          }}
        />
      ) : (
        <Button
          style={{ marginTop: 12 }}
          type="dashed"
          block
          icon={<PlusOutlined />}
          onClick={() => setCreating(true)}
        >
          {t('addChannelAccount')}
        </Button>
      )}
      <div className="muted small" style={{ marginTop: 10 }}>
        🔒 {t('secretHint')}
      </div>
    </Modal>
  )
}

function ChannelAccountForm({
  t,
  initial,
  onCancel,
  onSaved,
}: {
  t: T
  initial?: InboxChannel
  onCancel: () => void
  onSaved: () => Promise<void>
}) {
  const [kind, setKind] = useState(initial?.kind ?? 'telegram')
  const [name, setName] = useState(initial?.name ?? '')
  const [config, setConfig] = useState<Record<string, unknown>>(initial?.config ?? {})
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  // Known fields for the kind, plus any extra key the stored config already has
  // — so an operator can still see and edit a field the UI doesn't know about.
  const fields = useMemo(() => {
    const known = CHANNEL_CONFIG_FIELDS[kind] ?? []
    const extra = Object.keys(config)
      .filter((k) => !known.some((f) => f.key === k))
      .map((k) => ({ key: k, secret: isSecretField(k) }))
    return [...known, ...extra]
  }, [kind, config])

  async function save() {
    setBusy(true)
    setErr('')
    try {
      // Values still equal to the mask are dropped — the server treats a missing
      // key as "unchanged" the same way it treats the mask, and this keeps the
      // payload honest.
      const clean: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(config)) {
        if (v === '' || v === undefined) continue
        clean[k] = v
      }
      if (initial) await api.updateInboxChannel(initial.id, { name, config: clean })
      else await api.createInboxChannel({ kind, name, config: clean })
      await onSaved()
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="edit-inline">
      <div className="edit-inline-title">{initial ? t('editChannelAccount') : t('addChannelAccount')}</div>
      <div className="edit-grid">
        <Field label={t('channelKind')}>
          <Select
            value={kind}
            disabled={!!initial}
            onChange={(v) => {
              setKind(v)
              setConfig({})
            }}
            style={{ width: '100%' }}
            options={INBOX_CHANNEL_KINDS.map((k) => ({
              value: k,
              label: `${inboxChannelMeta(k).icon} ${tk(t, 'chan', k)}`,
            }))}
          />
        </Field>
        <Field label={t('channelName')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={tk(t, 'chan', kind)} />
        </Field>
        {fields.map((f) => {
          const value = String(config[f.key] ?? '')
          const secret = f.secret ?? isSecretField(f.key)
          return (
            <Field key={f.key} label={f.key} full={secret}>
              {secret ? (
                <Input.Password
                  value={value}
                  placeholder={initial ? SECRET_MASK : ''}
                  onChange={(e) => setConfig((c) => ({ ...c, [f.key]: e.target.value }))}
                />
              ) : (
                <Input
                  value={value}
                  onChange={(e) => setConfig((c) => ({ ...c, [f.key]: e.target.value }))}
                />
              )}
            </Field>
          )
        })}
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
