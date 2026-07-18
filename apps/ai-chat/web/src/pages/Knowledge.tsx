import { useCallback, useEffect, useState } from 'react'
import { Row, Col, Card, Select, Input, Button, Space, Switch, List, Typography, message, Upload } from 'antd'
import { InboxOutlined } from '@ant-design/icons'
import type { UploadProps } from 'antd'
import { api } from '../api'
import type { Bot } from '../api'
import type { T } from '../i18n'

export default function KnowledgePage({ t, bots }: { t: T; bots: Bot[] }) {
  const [botKey, setBotKey] = useState('')
  const bk = botKey || bots[0]?.key || ''
  const [text, setText] = useState('')
  const [wiki, setWiki] = useState(false)
  const [count, setCount] = useState<number | null>(null)
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<Array<{ name: string; summary: string; score: number }>>([])

  const loadCount = useCallback(() => {
    if (bk) api.botKnowledge(bk).then((r) => setCount(r.count)).catch(() => setCount(null))
  }, [bk])
  useEffect(loadCount, [loadCount])

  const write = async () => {
    try { await api.writeKnowledge(bk, text, wiki); setText(''); message.success(t('saved')); loadCount() }
    catch (e) { message.error(String(e)) }
  }

  const uploadProps: UploadProps = {
    multiple: true,
    showUploadList: false,
    customRequest: async (opt) => {
      try {
        const r = await api.uploadKnowledge(bk, opt.file as File)
        message.success(`✅ ${r.filename}`)
        opt.onSuccess?.(r)
        loadCount()
      } catch (e) {
        message.error(String(e))
        opt.onError?.(e as Error)
      }
    },
  }
  const search = async () => {
    try { const r = await api.searchKnowledge(bk, q); setHits(r.hits) } catch (e) { message.error(String(e)) }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Select style={{ width: 280 }} value={bk} onChange={setBotKey} options={bots.map((b) => ({ label: b.name, value: b.key }))} />
        <Typography.Text type="secondary" code>ai-chat:{bk}{count !== null ? ` · ${count} nodes` : ''}</Typography.Text>
      </Space>
      <Row gutter={16}>
        <Col span={12}>
          <Card title={'✍️ ' + t('writeKnowledge')} size="small">
            <Input.TextArea rows={5} value={text} onChange={(e) => setText(e.target.value)} placeholder={t('knowledgePlaceholder')} />
            <Space style={{ marginTop: 10 }}>
              <Switch checked={wiki} onChange={setWiki} /> {t('alsoWiki')}
              <Button type="primary" onClick={write} disabled={!text.trim()}>{t('writeKnowledge')}</Button>
            </Space>
            <Upload.Dragger {...uploadProps} style={{ marginTop: 14 }}>
              <p className="ant-upload-drag-icon"><InboxOutlined /></p>
              <p className="ant-upload-text">{t('uploadFile')}</p>
              <p className="ant-upload-hint">{t('uploadHint')}</p>
            </Upload.Dragger>
          </Card>
        </Col>
        <Col span={12}>
          <Card title={'🔎 ' + t('searchKnowledge')} size="small">
            <Input.Search value={q} onChange={(e) => setQ(e.target.value)} onSearch={search} enterButton={t('searchKnowledge')} />
            <List
              style={{ marginTop: 10 }}
              dataSource={hits}
              renderItem={(h) => <List.Item><List.Item.Meta title={h.name} description={h.summary} /></List.Item>}
            />
          </Card>
        </Col>
      </Row>
    </Space>
  )
}
