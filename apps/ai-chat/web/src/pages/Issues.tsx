import { useCallback, useEffect, useState } from 'react'
import {
  Table, Select, Input, Button, Space, Tag, Modal, Form, message, Typography,
  Drawer, Timeline, Descriptions, Divider, Empty,
} from 'antd'
import { PlusOutlined, HistoryOutlined, MessageOutlined } from '@ant-design/icons'
import { api } from '../api'
import type { Bot, Issue, Msg } from '../api'
import type { T } from '../i18n'

const STATUS_COLOR: Record<string, string> = { open: 'blue', in_progress: 'gold', resolved: 'green', closed: 'default' }
const PRIO_COLOR: Record<string, string> = { low: 'default', medium: 'blue', high: 'orange', urgent: 'red' }
const SENT_COLOR: Record<string, string> = { positive: 'green', neutral: 'default', negative: 'red' }
const STATUSES = ['open', 'in_progress', 'resolved', 'closed']
const PRIORITIES = ['low', 'medium', 'high', 'urgent']

type IssueEvent = { kind: string; field: string; oldVal: string; newVal: string; note: string; actor: string; createdAt: number }

export default function IssuesPage({ t, bots }: { t: T; bots: Bot[] }) {
  const [issues, setIssues] = useState<Issue[]>([])
  const [status, setStatus] = useState<string>()
  const [priority, setPriority] = useState<string>()
  const [bot, setBot] = useState<string>()
  const [search, setSearch] = useState('')
  const [creating, setCreating] = useState(false)
  const [cform] = Form.useForm()
  // Drawer detail
  const [detail, setDetail] = useState<{ issue: Issue; events: IssueEvent[] } | null>(null)
  const [linked, setLinked] = useState<Msg[]>([])
  const [form] = Form.useForm()

  const load = useCallback(() => {
    api.listIssues({ status, priority, bot, search }).then(setIssues).catch(() => setIssues([]))
  }, [status, priority, bot, search])
  useEffect(() => {
    load()
    const es = new EventSource('/api/events')
    es.onmessage = (e) => { try { const m = JSON.parse(e.data); if (String(m.type).startsWith('issue')) load() } catch { /* */ } }
    return () => es.close()
  }, [load])

  const openDetail = async (id: number) => {
    setLinked([])
    const d = await api.getIssue(id) as { issue: Issue; events: IssueEvent[] }
    setDetail(d)
    form.setFieldsValue(d.issue)
    if (d.issue.session_id) api.getSession(d.issue.session_id).then((s) => setLinked(s.messages)).catch(() => {})
  }
  const saveEdit = async (v: Record<string, string>) => {
    if (!detail) return
    try { await api.updateIssue(detail.issue.id, v); message.success(t('saved')); load(); openDetail(detail.issue.id) }
    catch (e) { message.error(String(e)) }
  }
  const doCreate = async (v: { title: string; botKey?: string; priority?: string; category?: string }) => {
    try { await api.createIssue(v); setCreating(false); cform.resetFields(); load() } catch (e) { message.error(String(e)) }
  }

  const columns = [
    { title: '#', dataIndex: 'id', width: 56 },
    { title: t('name'), dataIndex: 'title', render: (v: string, r: Issue) => v || <Typography.Text type="secondary">#{r.id}</Typography.Text> },
    { title: t('status'), dataIndex: 'status', render: (v: string) => <Tag color={STATUS_COLOR[v]}>{v}</Tag> },
    { title: t('priority'), dataIndex: 'priority', render: (v: string) => <Tag color={PRIO_COLOR[v]}>{v}</Tag> },
    { title: t('category'), dataIndex: 'category' },
    { title: t('sentiment'), dataIndex: 'sentiment', render: (v: string) => v ? <Tag color={SENT_COLOR[v]}>{v}</Tag> : null },
    { title: '', key: 'act', width: 90, render: (_: unknown, r: Issue) => <Button size="small" onClick={() => openDetail(r.id)}>{t('updateIssue')}</Button> },
  ]

  const fmt = (ms: number) => new Date(ms).toLocaleString()

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space wrap>
        <Select allowClear placeholder={t('status')} style={{ width: 140 }} value={status} onChange={setStatus}
          options={STATUSES.map((v) => ({ label: v, value: v }))} />
        <Select allowClear placeholder={t('priority')} style={{ width: 140 }} value={priority} onChange={setPriority}
          options={PRIORITIES.map((v) => ({ label: v, value: v }))} />
        <Select allowClear placeholder="bot" style={{ width: 160 }} value={bot} onChange={setBot}
          options={bots.map((b) => ({ label: b.name, value: b.key }))} />
        <Input.Search placeholder={t('searchKnowledge')} style={{ width: 220 }} value={search} onChange={(e) => setSearch(e.target.value)} onSearch={load} />
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreating(true)}>{t('newIssue')}</Button>
      </Space>
      <Table rowKey="id" size="small" columns={columns} dataSource={issues} locale={{ emptyText: t('noIssues') }} pagination={{ pageSize: 15 }}
        onRow={(r) => ({ onClick: () => openDetail(r.id), style: { cursor: 'pointer' } })} />

      {/* Detail drawer */}
      <Drawer width={560} title={detail?.issue.title || `#${detail?.issue.id}`} open={!!detail} onClose={() => setDetail(null)}>
        {detail && (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Descriptions column={2} size="small" bordered>
              <Descriptions.Item label={t('status')}><Tag color={STATUS_COLOR[detail.issue.status]}>{detail.issue.status}</Tag></Descriptions.Item>
              <Descriptions.Item label={t('priority')}><Tag color={PRIO_COLOR[detail.issue.priority]}>{detail.issue.priority}</Tag></Descriptions.Item>
              <Descriptions.Item label={t('category')}>{detail.issue.category || '—'}</Descriptions.Item>
              <Descriptions.Item label={t('sentiment')}>{detail.issue.sentiment ? <Tag color={SENT_COLOR[detail.issue.sentiment]}>{detail.issue.sentiment}</Tag> : '—'}</Descriptions.Item>
              {detail.issue.ai_summary && <Descriptions.Item label="AI" span={2}>{detail.issue.ai_summary}</Descriptions.Item>}
              {(detail.issue.tags || []).length > 0 && <Descriptions.Item label="Tags" span={2}>{(detail.issue.tags || []).map((tg) => <Tag key={tg}>{tg}</Tag>)}</Descriptions.Item>}
            </Descriptions>

            <Form form={form} layout="vertical" onFinish={saveEdit}>
              <div style={{ display: 'flex', gap: 16 }}>
                <Form.Item name="status" label={t('status')} style={{ flex: 1 }}>
                  <Select options={STATUSES.map((v) => ({ label: v, value: v }))} />
                </Form.Item>
                <Form.Item name="priority" label={t('priority')} style={{ flex: 1 }}>
                  <Select options={PRIORITIES.map((v) => ({ label: v, value: v }))} />
                </Form.Item>
              </div>
              <Form.Item name="category" label={t('category')}><Input /></Form.Item>
              <Form.Item name="assignee" label={t('assignee')}><Input /></Form.Item>
              <Form.Item name="resolution_note" label={t('resolutionNote')}><Input.TextArea rows={3} /></Form.Item>
              <Button type="primary" htmlType="submit">{t('save')}</Button>
            </Form>

            {linked.length > 0 && (
              <>
                <Divider orientation="left"><Space><MessageOutlined />{t('conversation')}</Space></Divider>
                <div style={{ maxHeight: 260, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 8 }}>
                  {linked.map((m) => (
                    <div key={m.id} style={{ display: 'flex', flexDirection: m.role === 'user' ? 'row-reverse' : 'row' }}>
                      <div className={`msg-bubble msg-bubble-${m.role === 'user' ? 'user' : m.role === 'operator' ? 'operator' : 'bot'}`} style={{ fontSize: 13 }}>{m.content}</div>
                    </div>
                  ))}
                </div>
              </>
            )}

            <Divider orientation="left"><Space><HistoryOutlined />{t('history')}</Space></Divider>
            {detail.events.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} /> : (
              <Timeline
                items={detail.events.map((ev) => ({
                  children: (
                    <div>
                      <Typography.Text>
                        {ev.kind === 'created' ? `Tạo ticket${ev.note ? ': ' + ev.note : ''}` : `${ev.field}: ${ev.oldVal || '—'} → ${ev.newVal || '—'}`}
                      </Typography.Text>
                      <br />
                      <Typography.Text type="secondary" style={{ fontSize: 11 }}>{ev.actor} · {fmt(ev.createdAt)}</Typography.Text>
                    </div>
                  ),
                }))}
              />
            )}
          </Space>
        )}
      </Drawer>

      {/* Create modal */}
      <Modal title={t('newIssue')} open={creating} onCancel={() => setCreating(false)} onOk={() => cform.submit()} okText={t('save')}>
        <Form form={cform} layout="vertical" onFinish={doCreate}>
          <Form.Item name="title" label={t('name')} rules={[{ required: true }]}><Input /></Form.Item>
          <Form.Item name="botKey" label="bot"><Select allowClear options={bots.map((b) => ({ label: b.name, value: b.key }))} /></Form.Item>
          <Form.Item name="priority" label={t('priority')} initialValue="medium">
            <Select options={PRIORITIES.map((v) => ({ label: v, value: v }))} />
          </Form.Item>
          <Form.Item name="category" label={t('category')}><Input /></Form.Item>
        </Form>
      </Modal>
    </Space>
  )
}
