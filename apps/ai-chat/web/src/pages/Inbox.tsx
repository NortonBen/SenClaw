import { useCallback, useEffect, useState } from 'react'
import {
  Row, Col, Card, Button, Input, Space, Tag, Descriptions, message, Rate, Typography,
  List, Avatar, Empty, Modal, Alert,
} from 'antd'
import {
  UserOutlined, CustomerServiceOutlined, RobotOutlined, SendOutlined, LineChartOutlined,
  RollbackOutlined, ContactsOutlined,
} from '@ant-design/icons'
import { api } from '../api'
import type { Bot, Msg, Session, SessionAnalysis } from '../api'
import type { T } from '../i18n'

const ICON: Record<string, string> = { telegram: '✈️', websocket: '🌐', zalo: '💙', facebook: '📘', tiktok: '🎵' }

function stateTag(t: T, s: string) {
  if (s === 'pending') return <Tag color="orange">{t('handoffPending')}</Tag>
  if (s === 'with_operator') return <Tag color="green">{t('withOperator')}</Tag>
  return <Tag>{t('botHandling')}</Tag>
}

export default function InboxPage({ t, bots }: { t: T; bots: Bot[] }) {
  const [sessions, setSessions] = useState<Session[]>([])
  const [selId, setSelId] = useState(0)
  const [detail, setDetail] = useState<{ session: Session; messages: Msg[] } | null>(null)
  const [reply, setReply] = useState('')
  const [analysis, setAnalysis] = useState<SessionAnalysis | null>(null)
  const [analyzing, setAnalyzing] = useState(false)

  const load = useCallback(() => { api.listSessions().then(setSessions).catch(() => {}) }, [])
  useEffect(() => {
    load()
    const es = new EventSource('/api/events')
    es.onmessage = () => load()
    return () => es.close()
  }, [load])
  useEffect(() => { if (selId) api.getSession(selId).then(setDetail).catch(() => {}) }, [selId, sessions])

  const botName = (k: string) => bots.find((b) => b.key === k)?.name || k
  const sendReply = async () => {
    if (!reply.trim() || !selId) return
    try { await api.handoffReply(selId, reply.trim()); setReply(''); api.getSession(selId).then(setDetail) }
    catch (e) { message.error(String(e)) }
  }
  const analyze = async () => {
    if (!selId) return
    setAnalyzing(true)
    try { setAnalysis(await api.analyzeSession(selId)) } catch (e) { message.error(String(e)) } finally { setAnalyzing(false) }
  }

  const crm = detail?.session.context?.crm && !detail.session.context.crm.none ? detail.session.context.crm : null

  return (
    <Row gutter={16} align="stretch" wrap={false} style={{ height: 'calc(100vh - 160px)' }}>
      {/* Session list */}
      <Col flex="300px" style={{ height: '100%' }}>
        <Card styles={{ body: { padding: 8, height: '100%', overflow: 'auto' } }} style={{ height: '100%' }}>
          <List
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('noSessions')} /> }}
            dataSource={sessions}
            renderItem={(s) => (
              <List.Item
                onClick={() => setSelId(s.id)}
                style={{ cursor: 'pointer', padding: '8px 10px', borderRadius: 8, background: selId === s.id ? 'rgba(24,144,255,.12)' : undefined }}
              >
                <List.Item.Meta
                  avatar={<Avatar>{ICON[s.channel_kind] || '💬'}</Avatar>}
                  title={<span style={{ fontSize: 13 }}>{s.customer_name || s.external_id}</span>}
                  description={<Space size={4}><Typography.Text type="secondary" style={{ fontSize: 11 }}>{botName(s.bot_key)}</Typography.Text>{stateTag(t, s.handoff_state)}</Space>}
                />
              </List.Item>
            )}
          />
        </Card>
      </Col>

      {/* Conversation */}
      <Col flex="auto" style={{ height: '100%', minWidth: 0 }}>
        <Card styles={{ body: { padding: 0, height: '100%', display: 'flex', flexDirection: 'column' } }} style={{ height: '100%' }}>
          {!detail ? (
            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <Empty description={t('noSessions')} />
            </div>
          ) : (
            <>
              {/* Header */}
              <div style={{ padding: '10px 16px', borderBottom: '1px solid var(--app-border)', display: 'flex', alignItems: 'center', gap: 8 }}>
                <Avatar>{ICON[detail.session.channel_kind] || '💬'}</Avatar>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Space size={6}><b>{detail.session.customer_name || detail.session.external_id}</b>{stateTag(t, detail.session.handoff_state)}</Space>
                </div>
                <Space>
                  <Button size="small" onClick={() => api.setHandoff(selId, 'with_operator').then(load)}>{t('handoffTake')}</Button>
                  <Button size="small" icon={<RollbackOutlined />} onClick={() => api.setHandoff(selId, 'bot').then(load)}>{t('handoffReturn')}</Button>
                  <Button size="small" type="primary" ghost icon={<LineChartOutlined />} loading={analyzing} onClick={analyze}>{t('analyze')}</Button>
                </Space>
              </div>

              {/* CRM banner */}
              {crm && (
                <Alert
                  type="success"
                  showIcon
                  icon={<ContactsOutlined />}
                  style={{ margin: '10px 16px 0', borderRadius: 8 }}
                  message={
                    <Space size={12} wrap>
                      <b>{crm.url ? <a href={crm.url} target="_blank" rel="noreferrer">{crm.name}</a> : crm.name}</b>
                      {crm.company && <span>🏢 {crm.company}</span>}
                      {crm.phone && <span>📞 {crm.phone}</span>}
                      {(crm.tags || []).map((tg) => <Tag key={tg} color="green" style={{ marginInlineEnd: 0 }}>{tg}</Tag>)}
                    </Space>
                  }
                />
              )}

              {/* Messages */}
              <div style={{ flex: 1, minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 10, padding: 16 }}>
                {detail.messages.map((m) => (
                  <div key={m.id} style={{ display: 'flex', gap: 8, flexDirection: m.role === 'user' ? 'row-reverse' : 'row' }}>
                    <Avatar size="small" style={{ background: m.role === 'user' ? undefined : m.role === 'operator' ? '#722ed1' : '#1890ff', flexShrink: 0 }}
                      icon={m.role === 'user' ? <UserOutlined /> : m.role === 'operator' ? <CustomerServiceOutlined /> : <RobotOutlined />} />
                    <div className={`msg-bubble msg-bubble-${m.role === 'user' ? 'user' : m.role === 'operator' ? 'operator' : 'bot'}`}>{m.content}</div>
                  </div>
                ))}
              </div>

              {/* Operator reply */}
              <div style={{ padding: 12, borderTop: '1px solid var(--app-border)' }}>
                <Space.Compact style={{ width: '100%' }}>
                  <Input value={reply} onChange={(e) => setReply(e.target.value)} onPressEnter={sendReply} placeholder={t('reply')} size="large" />
                  <Button type="primary" size="large" icon={<SendOutlined />} onClick={sendReply}>{t('send')}</Button>
                </Space.Compact>
              </div>
            </>
          )}
        </Card>
      </Col>

      {/* Analysis modal */}
      <Modal title={t('analyze')} open={!!analysis} footer={null} onCancel={() => setAnalysis(null)}>
        {analysis?.raw ? (
          <Typography.Paragraph>{analysis.summary}</Typography.Paragraph>
        ) : analysis ? (
          <Descriptions column={2} size="small" bordered>
            <Descriptions.Item label={t('sentiment')}>
              <Tag color={analysis.sentiment === 'negative' ? 'red' : analysis.sentiment === 'positive' ? 'green' : 'default'}>{analysis.sentiment}</Tag>
            </Descriptions.Item>
            <Descriptions.Item label={t('quality')}><Rate disabled value={analysis.quality} /></Descriptions.Item>
            <Descriptions.Item label={t('resolved')}>{analysis.resolved ? '✅' : '❌'}</Descriptions.Item>
            <Descriptions.Item label={t('category')}>{analysis.category}</Descriptions.Item>
            <Descriptions.Item label="Summary" span={2}>{analysis.summary}</Descriptions.Item>
            <Descriptions.Item label={t('suggestions')} span={2}>{analysis.suggestions}</Descriptions.Item>
          </Descriptions>
        ) : null}
      </Modal>
    </Row>
  )
}
