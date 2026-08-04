import { useEffect, useState } from 'react'
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Divider,
  Empty,
  Input,
  List,
  Select,
  Space,
  Tag,
  Typography,
} from 'antd'
import { GlobalOutlined, SearchOutlined } from '@ant-design/icons'
import { get, post, type Answer, type Doc } from './api'

export default function AskView() {
  const { message } = AntApp.useApp()
  const [docs, setDocs] = useState<Doc[]>([])
  const [scope, setScope] = useState<string[]>([])
  const [q, setQ] = useState('')
  const [busy, setBusy] = useState(false)
  const [ans, setAns] = useState<Answer | null>(null)

  useEffect(() => {
    get<Doc[]>('/docs').then(setDocs).catch(() => {})
  }, [])

  const run = async (external: boolean) => {
    if (!q.trim()) return message.warning('Chưa có câu hỏi')
    setBusy(true)
    setAns(null)
    try {
      setAns(
        await post<Answer>(external ? '/research' : '/ask', {
          question: q.trim(),
          doc_ids: scope,
        }),
      )
    } catch (e: any) {
      message.error(String(e.message ?? e), 8)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Card>
        <Space direction="vertical" style={{ width: '100%' }}>
          <Select
            mode="multiple"
            allowClear
            style={{ width: '100%' }}
            placeholder="Giới hạn trong tài liệu nào (bỏ trống = tất cả)"
            value={scope}
            onChange={setScope}
            options={docs.map((d) => ({ value: d.id, label: d.title }))}
          />
          <Input.TextArea
            rows={3}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Hỏi bất cứ điều gì trong tài liệu của bạn…"
          />
          <Space wrap>
            <Button type="primary" icon={<SearchOutlined />} loading={busy} onClick={() => run(false)}>
              Hỏi trong tài liệu
            </Button>
            <Button icon={<GlobalOutlined />} loading={busy} onClick={() => run(true)}>
              Mở rộng ra nguồn ngoài
            </Button>
          </Space>
        </Space>
      </Card>

      {ans && <AnswerCard a={ans} />}
      {!ans && !busy && <Empty description="Câu trả lời sẽ kèm trích dẫn [n] trỏ về đúng đoạn trong tài liệu" />}
    </Space>
  )
}

function AnswerCard({ a }: { a: Answer }) {
  return (
    <Card title={a.question}>
      {a.degraded && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="Chưa tổng hợp được bằng AI — đây là các đoạn liên quan nhất, đã đánh số như trích dẫn."
        />
      )}
      {a.notes.map((n, i) => (
        <Alert key={i} type="info" showIcon style={{ marginBottom: 8 }} message={n} />
      ))}

      <div className="study-prose">{a.answerMd}</div>

      <Divider titlePlacement="start">Bằng chứng</Divider>
      {a.sourcesUsed && a.sourcesUsed.length > 0 && (
        <Typography.Paragraph type="secondary">
          Nguồn ngoài đã hỏi: {a.sourcesUsed.join(', ')}
        </Typography.Paragraph>
      )}
      <List
        dataSource={a.evidence}
        renderItem={(e, i) => (
          <List.Item>
            <Space direction="vertical" size={2} style={{ width: '100%' }}>
              <Space wrap>
                <Tag color={e.kind === 'external' ? 'orange' : 'blue'}>[{i + 1}]</Tag>
                <b>{e.title}</b>
                {e.kind === 'external' ? (
                  <Tag color="orange">nguồn ngoài — chưa có trong tài liệu của bạn</Tag>
                ) : (
                  <Tag color="blue">tài liệu của bạn</Tag>
                )}
                {e.url && (
                  <a href={e.url} target="_blank" rel="noreferrer">
                    mở nguồn
                  </a>
                )}
                {e.charStart != null && (
                  <Typography.Text type="secondary">ký tự {e.charStart}–{e.charEnd}</Typography.Text>
                )}
              </Space>
              <div className="study-quote">
                <Typography.Text type="secondary">{e.text}</Typography.Text>
              </div>
            </Space>
          </List.Item>
        )}
      />
    </Card>
  )
}
