/** Hỏi đáp nghiệp vụ trên bộ tài liệu — trả lời kèm trích dẫn doc. */
import { useCallback, useEffect, useState } from 'react'
import { App, Button, Card, Empty, Input, Space, Tag, Typography } from 'antd'
import { SendOutlined } from '@ant-design/icons'
import MarkdownView from './md'
import { get, post, waitJob, fmtTime } from './api'

export default function AskPanel({
  projectId,
  onOpenDoc,
}: {
  projectId: number
  onOpenDoc: (id: number) => void
}) {
  const { message } = App.useApp()
  const [q, setQ] = useState('')
  const [asking, setAsking] = useState(false)
  const [history, setHistory] = useState<any[]>([])

  const load = useCallback(async () => {
    try {
      const r = await get(`/qa?project_id=${projectId}`)
      setHistory(r.qa ?? [])
    } catch {
      /* im lặng — panel phụ */
    }
  }, [projectId])

  useEffect(() => {
    load()
  }, [load])

  const ask = async () => {
    if (!q.trim()) return
    setAsking(true)
    try {
      const r = await post('/ask', { project: String(projectId), question: q })
      await waitJob(r.job_id)
      setQ('')
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e), 6)
    } finally {
      setAsking(false)
    }
  }

  return (
    <Card size="small" title="Hỏi đáp nghiệp vụ (trả lời từ tài liệu, kèm trích dẫn)">
      <Space.Compact style={{ width: '100%' }}>
        <Input
          placeholder="Vd: đăng nhập sai bao nhiêu lần thì khóa? luồng hoàn tiền chạy thế nào?"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onPressEnter={ask}
          disabled={asking}
        />
        <Button type="primary" icon={<SendOutlined />} loading={asking} onClick={ask}>
          Hỏi
        </Button>
      </Space.Compact>
      <div style={{ marginTop: 14 }}>
        {history.length === 0 && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có câu hỏi nào" />}
        {history.map((h, i) => (
          <Card key={i} size="small" style={{ marginBottom: 10 }}>
            <Typography.Text strong>❓ {h.question}</Typography.Text>
            <Typography.Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>
              {fmtTime(h.created_at)}
            </Typography.Text>
            <div style={{ marginTop: 6 }}>
              <MarkdownView md={h.answer} />
            </div>
            <Space size={4} wrap style={{ marginTop: 4 }}>
              {(h.citations ?? []).map((c: any) => (
                <Tag key={c.doc_id} style={{ cursor: 'pointer' }} onClick={() => onOpenDoc(c.doc_id)}>
                  📄 {c.title}
                </Tag>
              ))}
            </Space>
          </Card>
        ))}
      </div>
    </Card>
  )
}
