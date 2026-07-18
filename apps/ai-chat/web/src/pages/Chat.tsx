import { useCallback, useEffect, useRef, useState } from 'react'
import { Row, Col, Card, Select, Input, Button, Space, Avatar, List, Empty, Typography, Popconfirm, Tag, Modal, Segmented, message, Form } from 'antd'
import {
  RobotOutlined, UserOutlined, CustomerServiceOutlined, SendOutlined, PlusOutlined, DeleteOutlined,
  ContactsOutlined, UserAddOutlined,
} from '@ant-design/icons'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { api } from '../api'
import type { Bot, Channel, Conversation, CrmCustomer } from '../api'
import type { T } from '../i18n'

type Line = { role: 'user' | 'assistant' | 'operator'; text: string }
type Active = { ext: string; name: string; kind: string; sessionId: number } | null

const ICON: Record<string, string> = { telegram: '✈️', websocket: '🌐', zalo: '💙', facebook: '📘', tiktok: '🎵' }

function relTime(ms: number): string {
  const s = Math.floor((Date.now() - ms) / 1000)
  if (s < 60) return 'vừa xong'
  if (s < 3600) return `${Math.floor(s / 60)} phút`
  if (s < 86400) return `${Math.floor(s / 3600)} giờ`
  return `${Math.floor(s / 86400)} ngày`
}

export default function ChatPage({ t, bots }: { t: T; bots: Bot[] }) {
  const [botKey, setBotKey] = useState('')
  const bk = botKey || bots[0]?.key || ''
  const [convos, setConvos] = useState<Conversation[]>([])
  const [channels, setChannels] = useState<Channel[]>([])
  const [active, setActive] = useState<Active>(null)
  const [lines, setLines] = useState<Line[]>([])
  const [input, setInput] = useState('')
  const [newOpen, setNewOpen] = useState(false)
  const wsRef = useRef<WebSocket | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)

  const loadConvos = useCallback(() => {
    if (bk) api.listConversations(bk, '').then(setConvos).catch(() => setConvos([]))
  }, [bk])
  useEffect(loadConvos, [loadConvos])
  useEffect(() => {
    if (bk) api.listChannels(bk).then((c) => setChannels(c.filter((x) => x.enabled))).catch(() => setChannels([]))
    setActive(null); setLines([])
  }, [bk])

  // Web conversations run over the live WebSocket (the server replays the
  // transcript on connect). Other platforms are REST: load the transcript and
  // send outbound through the channel.
  useEffect(() => {
    if (!bk || !active) { setLines([]); return }
    if (active.kind !== 'websocket') {
      api.getSession(active.sessionId)
        .then((d) => setLines(d.messages.map((m) => ({
          role: m.role === 'user' ? 'user' : m.role === 'operator' ? 'operator' : 'assistant',
          text: m.content,
        }))))
        .catch(() => setLines([]))
      return
    }
    setLines([])
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const nameQ = active.name ? `&name=${encodeURIComponent(active.name)}` : ''
    const ws = new WebSocket(`${proto}://${location.host}/api/ws/chat/${active.ext}?bot=${bk}${nameQ}`)
    wsRef.current = ws
    ws.onmessage = (ev) => {
      try {
        const m = JSON.parse(ev.data)
        if (m.type === 'history') {
          setLines((m.messages as Array<{ role: string; content: string }>).map((x) => ({
            role: x.role === 'user' ? 'user' : x.role === 'operator' ? 'operator' : 'assistant',
            text: x.content,
          })))
        } else if (m.type === 'chat_response') setLines((l) => [...l, { role: 'assistant', text: m.text }])
        else if (m.type === 'operator_message') setLines((l) => [...l, { role: 'operator', text: m.text }])
        else if (m.type === 'handoff') setLines((l) => [...l, { role: 'operator', text: `— ${m.state} —` }])
      } catch { /* ignore */ }
    }
    return () => ws.close()
  }, [bk, active])

  useEffect(() => endRef.current?.scrollIntoView({ behavior: 'smooth' }), [lines])

  const send = async () => {
    const text = input.trim()
    if (!text || !active) return
    if (active.kind === 'websocket') {
      if (wsRef.current?.readyState !== WebSocket.OPEN) return
      wsRef.current.send(JSON.stringify({ type: 'chat_message', text }))
      setLines((l) => [...l, { role: 'user', text }])
    } else {
      // Real platform: we speak to the customer as the shop.
      try {
        await api.conversationSend(active.sessionId, text)
        setLines((l) => [...l, { role: 'operator', text }])
      } catch (e) { message.error(String(e)); return }
    }
    setInput('')
    setTimeout(loadConvos, 1500)
  }

  const del = async (c: Conversation) => {
    await api.deleteSession(c.id)
    if (c.externalId === active?.ext) { setActive(null); setLines([]) }
    loadConvos()
  }

  const avatar = (r: Line['role']) =>
    r === 'user' ? <Avatar icon={<UserOutlined />} /> : r === 'operator'
      ? <Avatar style={{ background: '#722ed1' }} icon={<CustomerServiceOutlined />} />
      : <Avatar style={{ background: '#1890ff' }} icon={<RobotOutlined />} />

  return (
    <Row gutter={16} align="stretch" wrap={false} style={{ height: 'calc(100vh - 160px)' }}>
      <Col flex="300px" style={{ height: '100%' }}>
        <Card styles={{ body: { padding: 12, height: '100%', display: 'flex', flexDirection: 'column' } }} style={{ height: '100%' }}>
          <Select style={{ width: '100%', marginBottom: 10 }} value={bk} onChange={setBotKey}
            options={bots.map((b) => ({ label: b.name, value: b.key }))} />
          <Button type="primary" icon={<PlusOutlined />} block onClick={() => setNewOpen(true)} style={{ marginBottom: 10 }}>
            {t('newConversation')}
          </Button>
          <div style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
            <List
              locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('noConversations')} /> }}
              dataSource={convos}
              renderItem={(c) => (
                <List.Item
                  onClick={() => setActive({ ext: c.externalId, name: c.customerName, kind: c.channelKind, sessionId: c.id })}
                  style={{ cursor: 'pointer', padding: '8px 10px', borderRadius: 8, background: c.externalId === active?.ext ? 'rgba(24,144,255,.12)' : undefined }}
                  actions={[
                    <Popconfirm key="d" title={t('delete') + '?'} onConfirm={() => del(c)}>
                      <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={(e) => e.stopPropagation()} />
                    </Popconfirm>,
                  ]}
                >
                  <List.Item.Meta
                    avatar={<Avatar style={{ background: c.externalId === active?.ext ? '#1890ff' : '#8c8c8c' }}>{ICON[c.channelKind] || '💬'}</Avatar>}
                    title={<Space size={6}><span style={{ fontSize: 13 }}>{c.customerName || `#${c.id}`}</span><Tag style={{ marginInlineEnd: 0 }}>{c.messageCount}</Tag></Space>}
                    description={<Typography.Text type="secondary" ellipsis style={{ fontSize: 12, maxWidth: 170, display: 'inline-block' }}>{c.preview || '…'}</Typography.Text>}
                  />
                  <Typography.Text type="secondary" style={{ fontSize: 11 }}>{relTime(c.lastActivity)}</Typography.Text>
                </List.Item>
              )}
            />
          </div>
        </Card>
      </Col>

      <Col flex="auto" style={{ height: '100%', minWidth: 0 }}>
        <Card styles={{ body: { padding: 0, height: '100%', display: 'flex', flexDirection: 'column' } }} style={{ height: '100%' }}>
          {!active ? (
            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <Empty description={t('pickConversation')}>
                <Button type="primary" icon={<PlusOutlined />} onClick={() => setNewOpen(true)}>{t('newConversation')}</Button>
              </Empty>
            </div>
          ) : (
            <>
              <div style={{ padding: '10px 16px', borderBottom: '1px solid var(--app-border)' }}>
                <Space>
                  <Avatar size="small">{ICON[active.kind] || '💬'}</Avatar>
                  <b>{active.name || active.ext}</b>
                  <Tag>{active.kind}</Tag>
                  {active.kind !== 'websocket' && <Tag color="purple">{t('outbound')}</Tag>}
                </Space>
              </div>
              <div style={{ flex: 1, minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 10, padding: 16 }}>
                {lines.map((l, i) => (
                  <div key={i} style={{ display: 'flex', gap: 8, flexDirection: l.role === 'user' ? 'row-reverse' : 'row' }}>
                    {avatar(l.role)}
                    <div className={`msg-bubble msg-bubble-${l.role === 'assistant' ? 'bot' : l.role}`}>
                      {l.role === 'assistant' ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{l.text}</ReactMarkdown> : l.text}
                    </div>
                  </div>
                ))}
                <div ref={endRef} />
              </div>
              <div style={{ padding: 12, borderTop: '1px solid var(--app-border)' }}>
                <Space.Compact style={{ width: '100%' }}>
                  <Input value={input} onChange={(e) => setInput(e.target.value)} onPressEnter={send} size="large"
                    placeholder={active.kind === 'websocket' ? t('typeMessage') : t('sendToCustomer')} />
                  <Button type="primary" size="large" icon={<SendOutlined />} onClick={send}>{t('send')}</Button>
                </Space.Compact>
              </div>
            </>
          )}
        </Card>
      </Col>

      <NewConvModal
        open={newOpen} onClose={() => setNewOpen(false)} t={t} bot={bk} channels={channels}
        onCreated={(a) => { setActive(a); setNewOpen(false); loadConvos() }}
      />
    </Row>
  )
}

/** Start a conversation: pick the platform (channel) + who you're talking to. */
function NewConvModal({ open, onClose, t, bot, channels, onCreated }: {
  open: boolean; onClose: () => void; t: T; bot: string; channels: Channel[]; onCreated: (a: Active) => void
}) {
  const [mode, setMode] = useState<'guest' | 'crm'>('guest')
  const [channelId, setChannelId] = useState<number>()
  const [opts, setOpts] = useState<CrmCustomer[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [extId, setExtId] = useState('')

  const ch = channels.find((c) => c.id === channelId) || channels[0]
  const web = !ch || ch.kind === 'websocket'
  useEffect(() => { if (open && channels.length && !channelId) setChannelId(channels[0].id) }, [open, channels, channelId])
  // Re-check reachability whenever the target channel changes.
  useEffect(() => { if (open && mode === 'crm') search('') }, [open, mode, ch?.kind])

  const search = (q: string) => {
    setLoading(true)
    api.crmSearch(q, ch?.kind).then(setOpts).catch(() => setOpts([])).finally(() => setLoading(false))
  }

  const create = async (crmCustomerId?: number) => {
    if (!ch) { message.error(t('noChannel')); return }
    // Validate the customer is actually reachable on this channel before creating.
    if (crmCustomerId && !web) {
      const c = opts.find((x) => x.id === crmCustomerId)
      if (c && !c.reachable && !extId.trim()) {
        message.error(`${c.name}: ${t('notReachable')} ${ch.kind}`)
        return
      }
    }
    setBusy(true)
    try {
      const r = await api.createConversation({
        bot, channelId: ch.id, crmCustomerId,
        externalId: extId.trim() || undefined,
        name: crmCustomerId ? undefined : 'Khách web',
      })
      onCreated({ ext: r.externalId, name: r.customerName, kind: r.channelKind, sessionId: r.sessionId })
      setExtId('')
    } catch (e) { message.error(String(e)) } finally { setBusy(false) }
  }

  return (
    <Modal title={t('newConversation')} open={open} onCancel={onClose} footer={null}>
      <Form layout="vertical">
        {/* WHICH platform / channel to chat on */}
        <Form.Item label={t('chatVia')} style={{ marginBottom: 12 }}>
          <Select
            value={ch?.id}
            onChange={setChannelId}
            placeholder={t('noChannel')}
            options={channels.map((c) => ({ value: c.id, label: `${ICON[c.kind] || '💬'} ${c.name || c.kind} (${c.kind})` }))}
          />
        </Form.Item>
      </Form>
      <Segmented
        block
        value={mode}
        onChange={(v) => setMode(v as 'guest' | 'crm')}
        options={[
          { label: <Space><UserAddOutlined />{t('guestCustomer')}</Space>, value: 'guest' },
          { label: <Space><ContactsOutlined />{t('fromCrm')}</Space>, value: 'crm' },
        ]}
        style={{ marginBottom: 16 }}
      />
      {/* A real platform needs the customer's id there (CRM supplies it when known). */}
      {!web && (
        <Form layout="vertical">
          <Form.Item label={`${t('customerIdOn')} ${ch?.kind}`} style={{ marginBottom: 12 }}
            extra={mode === 'crm' ? t('crmIdHint') : undefined}>
            <Input value={extId} onChange={(e) => setExtId(e.target.value)} placeholder={ch?.kind === 'telegram' ? 'chat id' : 'id / số điện thoại'} />
          </Form.Item>
        </Form>
      )}
      {mode === 'guest' ? (
        <Button type="primary" block icon={<PlusOutlined />} loading={busy} onClick={() => create()}>{t('startChat')}</Button>
      ) : (
        <Select
          showSearch
          style={{ width: '100%' }}
          placeholder={t('searchCrm')}
          filterOption={false}
          loading={loading || busy}
          onSearch={search}
          onChange={(v) => create(Number(v))}
          notFoundContent={loading ? '…' : t('noCustomers')}
          options={opts.map((c) => {
            // Only customers with an id on this channel can be contacted there
            // (a manually typed id overrides).
            const ok = web || c.reachable || !!extId.trim()
            return {
              value: c.id,
              disabled: !ok,
              label: (
                <Space size={6}>
                  <span>{c.name}</span>
                  {c.company && <Typography.Text type="secondary">· {c.company}</Typography.Text>}
                  {web ? null : c.reachable
                    ? <Tag color="green" style={{ marginInlineEnd: 0 }}>{ICON[ch!.kind] || '💬'} {c.channelValue}</Tag>
                    : <Tag color="default" style={{ marginInlineEnd: 0 }}>{t('notReachable')} {ch?.kind}</Tag>}
                </Space>
              ),
            }
          })}
        />
      )}
    </Modal>
  )
}
