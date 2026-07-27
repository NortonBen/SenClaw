import { useCallback, useEffect, useState } from 'react'
import { App, Button, Card, Input, Modal, Select, Table, Tag } from 'antd'
import { ago, getInbox, mutate, type InboxMsg } from '../api'

export default function Inbox() {
  const { message } = App.useApp()
  const [msgs, setMsgs] = useState<InboxMsg[]>([])
  const [platform, setPlatform] = useState<string>('')
  const [reply, setReply] = useState<InboxMsg | null>(null)
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)

  const load = useCallback(async () => setMsgs((await getInbox())?.messages ?? []), [])

  useEffect(() => {
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [load])

  const platforms = Array.from(new Set(msgs.map((m) => m.platform)))
  const rows = msgs.filter((m) => !platform || m.platform === platform)

  const send = async () => {
    if (!reply || !text.trim()) return
    setSending(true)
    const r = await mutate('/api/inbox/reply', 'POST', {
      platform: reply.platform,
      external_id: reply.external_id,
      text,
    })
    setSending(false)
    if (r.ok) {
      message.success(r.data?.drafted ? 'Đã tạo nháp chờ duyệt' : 'Đã gửi')
      setReply(null)
      setText('')
      load()
    } else message.error(r.error ?? 'Lỗi')
  }

  return (
    <Card
      size="small"
      extra={
        <Select
          allowClear
          placeholder="Lọc nền tảng"
          style={{ width: 170 }}
          value={platform || undefined}
          onChange={(v) => setPlatform(v ?? '')}
          options={platforms.map((p) => ({ value: p, label: p }))}
        />
      }
    >
      <Table<InboxMsg>
        size="small"
        rowKey="id"
        dataSource={rows}
        pagination={{ pageSize: 20, hideOnSinglePage: true }}
        locale={{
          emptyText: 'Chưa có tin nhắn. Tin đến chỉ xuất hiện sau khi extension thu được (social_inbox_poll).',
        }}
        columns={[
          { title: 'Nền tảng', dataIndex: 'platform', width: 110 },
          {
            title: 'Chiều',
            dataIndex: 'direction',
            width: 90,
            render: (v: string) => <Tag color={v === 'in' ? 'blue' : 'green'}>{v === 'in' ? 'đến' : 'đi'}</Tag>,
          },
          { title: 'Người gửi', dataIndex: 'sender', width: 150, render: (v: string) => v || '—' },
          { title: 'Nội dung', dataIndex: 'text' },
          {
            title: 'Thread',
            dataIndex: 'external_id',
            width: 110,
            render: (v: string) => <span className="mono">{v}</span>,
          },
          {
            title: 'Lúc',
            dataIndex: 'created_at',
            width: 90,
            render: (v: string) => <span className="mono">{ago(v)}</span>,
          },
          {
            title: '',
            width: 90,
            render: (_, r) =>
              r.direction === 'in' && (
                <Button size="small" onClick={() => setReply(r)}>
                  Trả lời
                </Button>
              ),
          },
        ]}
      />

      <Modal
        open={!!reply}
        title={`Trả lời ${reply?.platform} / ${reply?.external_id}`}
        onCancel={() => setReply(null)}
        onOk={send}
        confirmLoading={sending}
        okText="Gửi"
        cancelText="Huỷ"
      >
        <p style={{ opacity: 0.7 }}>
          <b>{reply?.sender || 'Khách'}:</b> {reply?.text}
        </p>
        <Input.TextArea
          rows={4}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Nội dung trả lời…"
        />
        <p style={{ fontSize: 12, opacity: 0.65, marginTop: 8, marginBottom: 0 }}>
          Ở chế độ <b>draft</b>, câu trả lời sẽ thành nháp chờ duyệt thay vì gửi ngay.
        </p>
      </Modal>
    </Card>
  )
}
