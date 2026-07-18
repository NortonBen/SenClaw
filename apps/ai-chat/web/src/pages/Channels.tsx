import { useCallback, useEffect, useState } from 'react'
import type { ChangeEvent } from 'react'
import { Table, Select, Button, Input, Space, Tag, Form, message, Popconfirm, Switch, Modal } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import { api } from '../api'
import type { Bot, Channel } from '../api'
import type { T } from '../i18n'

const CHANNEL_FIELDS: Record<string, string[]> = {
  telegram: ['token'],
  websocket: [],
  zalo: ['app_id', 'app_secret', 'access_token', 'refresh_token', 'oa_id'],
  facebook: ['page_id', 'access_token'],
  tiktok: ['app_key', 'app_secret', 'access_token', 'shop_cipher'],
}
const ICON: Record<string, string> = { telegram: '✈️', websocket: '🌐', zalo: '💙', facebook: '📘', tiktok: '🎵' }

export default function ChannelsPage({ t, bots }: { t: T; bots: Bot[] }) {
  const [botKey, setBotKey] = useState('')
  const bk = botKey || bots[0]?.key || ''
  const [channels, setChannels] = useState<Channel[]>([])
  const [kind, setKind] = useState('telegram')
  const [config, setConfig] = useState<Record<string, string>>({})
  const [name, setName] = useState('')
  const [addOpen, setAddOpen] = useState(false)

  const load = useCallback(() => {
    if (bk) api.listChannels(bk).then(setChannels).catch(() => setChannels([]))
  }, [bk])
  useEffect(load, [load])

  const add = async () => {
    try {
      await api.createChannel({ botKey: bk, kind, name: name || kind, config })
      setConfig({}); setName(''); setAddOpen(false)
      load()
    } catch (e) {
      message.error(String(e))
    }
  }
  const test = async (id: number) => {
    const r = await api.testChannel(id)
    r.ok ? message.success(r.message) : message.warning(r.message)
  }

  const columns = [
    { title: t('channelKind'), dataIndex: 'kind', render: (k: string) => <Space>{ICON[k]} {k}{k === 'tiktok' && <Tag color="orange">{t('experimental')}</Tag>}</Space> },
    { title: t('name'), dataIndex: 'name' },
    {
      title: t('status'), dataIndex: 'enabled',
      render: (_: boolean, c: Channel) => (
        <Space>
          {c.enabled ? <Tag color="green">on</Tag> : <Tag>off</Tag>}
          {c.lastError ? <Tag color="red">{c.lastError}</Tag> : c.lastStatus ? <Tag>{c.lastStatus}</Tag> : null}
        </Space>
      ),
    },
    {
      title: '', key: 'act',
      render: (_: unknown, c: Channel) => (
        <Space>
          <Button size="small" onClick={() => test(c.id)}>{t('test')}</Button>
          <Switch size="small" checked={c.enabled} onChange={(v) => api.updateChannel(c.id, { enabled: v }).then(load)} />
          <Popconfirm title={t('delete') + '?'} onConfirm={() => api.deleteChannel(c.id).then(load)}>
            <Button size="small" danger>✕</Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Select style={{ width: 280 }} value={bk} onChange={setBotKey} options={bots.map((b) => ({ label: b.name, value: b.key }))} />
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setAddOpen(true)}>{t('addChannel')}</Button>
      </Space>
      <Table rowKey="id" size="small" columns={columns} dataSource={channels} pagination={false} />

      <Modal title={t('addChannel')} open={addOpen} onCancel={() => setAddOpen(false)} onOk={add} okText={t('addChannel')}>
        <Form layout="vertical">
          <Form.Item label={t('channelKind')}>
            <Select value={kind} onChange={(v) => { setKind(v); setConfig({}) }}
              options={Object.keys(CHANNEL_FIELDS).map((k) => ({ label: `${ICON[k]} ${k}`, value: k }))} />
          </Form.Item>
          <Form.Item label={t('name')}>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={kind} />
          </Form.Item>
          {CHANNEL_FIELDS[kind].map((fld) => {
            const secret = fld.includes('secret') || fld.includes('token')
            const props = { value: config[fld] || '', onChange: (e: ChangeEvent<HTMLInputElement>) => setConfig({ ...config, [fld]: e.target.value }) }
            return (
              <Form.Item key={fld} label={fld}>
                {secret ? <Input.Password {...props} /> : <Input {...props} />}
              </Form.Item>
            )
          })}
          {CHANNEL_FIELDS[kind].length === 0 && <Tag color="blue">Web chat không cần cấu hình</Tag>}
        </Form>
      </Modal>
    </Space>
  )
}
