import { useState } from 'react'
import { App, Button, Card, Col, Dropdown, Input, Progress, Row, Space, Statistic, Table, Tag, Typography } from 'antd'
import { HeartOutlined, LikeOutlined, MoreOutlined } from '@ant-design/icons'
import { api, type CachedComment, type CommentStats } from '../api'

const SENTIMENT_COLOR: Record<string, string> = { pos: 'success', neg: 'error', neu: 'default' }

/// A labelled breakdown ({ key: count }) rendered as ranked progress bars.
function Breakdown({ title, data }: { title: string; data: Record<string, number> }) {
  const entries = Object.entries(data).sort((a, b) => b[1] - a[1])
  const max = Math.max(1, ...entries.map(([, v]) => v))
  return (
    <Card size="small" title={title} styles={{ body: { paddingTop: 8 } }}>
      {entries.length === 0 && <Typography.Text type="secondary">Chưa có dữ liệu.</Typography.Text>}
      {entries.map(([k, v]) => (
        <div key={k} style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '4px 0' }}>
          <span style={{ flex: '0 0 34%', textAlign: 'right', fontSize: 12, opacity: 0.75, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {k}
          </span>
          <Progress percent={Math.round((v / max) * 100)} showInfo={false} size="small" style={{ flex: 1, margin: 0 }} />
          <span style={{ flex: '0 0 2.5em' }}>{v}</span>
        </div>
      ))}
    </Card>
  )
}

export function DashboardPage({ connected }: { connected: boolean }) {
  const { message } = App.useApp()
  const [videoId, setVideoId] = useState('')
  const [stats, setStats] = useState<CommentStats | null>(null)
  const [comments, setComments] = useState<CachedComment[]>([])
  const [loading, setLoading] = useState(false)

  const load = async (vid: string) => {
    const [st, cc] = await Promise.all([api.commentStats(vid), api.cachedComments(vid)])
    setStats(st)
    setComments(cc.comments)
  }

  const syncAnalyse = async () => {
    const v = videoId.trim()
    if (!v) return
    setLoading(true)
    try {
      const s = await api.syncComments(v)
      const a = await api.analyzeComments()
      await load(v)
      message.success(`Đồng bộ ${s.fetched} (mới ${s.new}), phân tích ${a.analyzed}`)
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }

  const act = async (c: CachedComment, action: string) => {
    try {
      await api.commentAction(c.id, action)
      message.success(`Đã ${action}`)
    } catch (e) {
      message.error(String(e))
    }
  }

  const moderate = async (c: CachedComment, status: string, ban = false) => {
    try {
      await api.moderate(c.id, status, ban)
      message.success(`Đã kiểm duyệt: ${status}`)
      if (videoId.trim()) await load(videoId.trim())
    } catch (e) {
      message.error(String(e))
    }
  }

  const columns = [
    {
      title: 'Bình luận',
      dataIndex: 'text',
      render: (_: string, c: CachedComment) => (
        <div>
          <div>{c.text}</div>
          <div style={{ fontSize: 12, opacity: 0.6 }}>
            {c.author}
            {c.sentiment && (
              <Tag color={SENTIMENT_COLOR[c.sentiment] || 'default'} style={{ marginLeft: 6 }}>
                {c.sentiment}
              </Tag>
            )}
            {c.intent && <span> · {c.intent}</span>}
          </div>
        </div>
      ),
    },
    { title: '👍', dataIndex: 'like_count', width: 70, render: (n: number | null) => n ?? '' },
    {
      title: '',
      width: 130,
      render: (_: unknown, c: CachedComment) => (
        <Space>
          <Button size="small" icon={<HeartOutlined />} onClick={() => act(c, 'heart')} />
          <Button size="small" icon={<LikeOutlined />} onClick={() => act(c, 'like')} />
          <Dropdown
            menu={{
              items: [
                { key: 'pin', label: 'Ghim' },
                { key: 'reject', label: 'Từ chối (moderation)', danger: true },
                { key: 'held', label: 'Giữ để duyệt (moderation)' },
              ],
              onClick: ({ key }) => {
                if (key === 'pin') act(c, 'pin')
                else if (key === 'reject') moderate(c, 'rejected')
                else if (key === 'held') moderate(c, 'heldForReview')
              },
            }}
          >
            <Button size="small" icon={<MoreOutlined />} />
          </Dropdown>
        </Space>
      ),
    },
  ]

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Typography.Title level={4} style={{ margin: 0 }}>
        Bình luận & thống kê
      </Typography.Title>
      <Space.Compact style={{ width: '100%', maxWidth: 560 }}>
        <Input
          placeholder="Video ID (vd dQw4w9WgXcQ)…"
          value={videoId}
          onChange={(e) => setVideoId(e.target.value)}
          onPressEnter={syncAnalyse}
          allowClear
        />
        <Button type="primary" loading={loading} disabled={!connected} onClick={syncAnalyse}>
          Đồng bộ & phân tích
        </Button>
      </Space.Compact>
      {!connected && <Typography.Text type="warning">Cần kết nối extension (tab Cài đặt).</Typography.Text>}

      {stats && (
        <>
          <Row gutter={16}>
            <Col span={6}><Card size="small"><Statistic title="Bình luận" value={stats.total} /></Card></Col>
            <Col span={6}><Card size="small"><Statistic title="Đã phân tích" value={stats.analyzed} /></Card></Col>
            <Col span={6}><Card size="small"><Statistic title="Spam" value={stats.spam} /></Card></Col>
            <Col span={6}>
              <Card size="small">
                <Statistic title="Cảm xúc TB" value={stats.avgSentiment ?? 0} precision={2} />
              </Card>
            </Col>
          </Row>
          <Row gutter={16}>
            <Col xs={24} md={8}><Breakdown title="Cảm xúc" data={stats.sentiment} /></Col>
            <Col xs={24} md={8}><Breakdown title="Ý định" data={stats.intent} /></Col>
            <Col xs={24} md={8}><Breakdown title="Người bình luận nhiều nhất" data={stats.topAuthors} /></Col>
          </Row>
        </>
      )}

      <Table
        rowKey="id"
        size="middle"
        columns={columns}
        dataSource={comments}
        pagination={{ pageSize: 15, showSizeChanger: true, showTotal: (t) => `${t} bình luận` }}
        locale={{ emptyText: 'Chưa có bình luận nào được đồng bộ.' }}
      />
    </Space>
  )
}
