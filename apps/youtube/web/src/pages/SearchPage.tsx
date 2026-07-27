import { useState } from 'react'
import { App, Button, Input, Space, Table, Typography } from 'antd'
import { EditOutlined } from '@ant-design/icons'
import { api, type VideoItem } from '../api'
import type { View } from '../App'

export function SearchPage({ connected, onView }: { connected: boolean; onView: (v: View) => void }) {
  const { message } = App.useApp()
  const [q, setQ] = useState('')
  const [items, setItems] = useState<VideoItem[]>([])
  const [loading, setLoading] = useState(false)

  const run = async () => {
    if (!q.trim()) return
    setLoading(true)
    try {
      const r = await api.search(q.trim())
      setItems(r.items)
      if (!r.items.length) message.info('Không có kết quả.')
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }

  const draftFor = async (v: VideoItem) => {
    try {
      await api.aiDraft('comment', `Video: ${v.title} — kênh ${v.channel}`, v.videoId)
      message.success('Đã tạo bản nháp bình luận')
      onView('drafts')
    } catch (e) {
      message.error(String(e))
    }
  }

  const columns = [
    {
      title: 'Video',
      dataIndex: 'title',
      render: (_: string, v: VideoItem) => (
        <div>
          <a href={`https://www.youtube.com/watch?v=${v.videoId}`} target="_blank" rel="noreferrer">
            {v.title || v.videoId}
          </a>
          <div style={{ fontSize: 12, opacity: 0.6 }}>{v.channel}</div>
        </div>
      ),
    },
    { title: 'Lượt xem', dataIndex: 'views', width: 130, responsive: ['md' as const] },
    { title: 'Đăng', dataIndex: 'published', width: 130, responsive: ['md' as const] },
    {
      title: '',
      width: 150,
      render: (_: unknown, v: VideoItem) => (
        <Button size="small" icon={<EditOutlined />} onClick={() => draftFor(v)}>
          Soạn bình luận
        </Button>
      ),
    },
  ]

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Typography.Title level={4} style={{ margin: 0 }}>
        Tìm kiếm
      </Typography.Title>
      <Space.Compact style={{ width: '100%' }}>
        <Input
          placeholder="Tìm video trên YouTube…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onPressEnter={run}
          allowClear
        />
        <Button type="primary" loading={loading} disabled={!connected} onClick={run}>
          Tìm
        </Button>
      </Space.Compact>
      {!connected && (
        <Typography.Text type="warning">Cần kết nối extension để tìm kiếm (xem tab Cài đặt).</Typography.Text>
      )}
      <Table
        rowKey="videoId"
        size="middle"
        columns={columns}
        dataSource={items}
        loading={loading}
        pagination={{ pageSize: 10, showSizeChanger: true, showTotal: (t) => `${t} video` }}
        locale={{ emptyText: 'Chưa có kết quả.' }}
      />
    </Space>
  )
}
