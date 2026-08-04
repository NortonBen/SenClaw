// Tab Trang cá nhân: liệt kê video mới nhất của một tài khoản (best-effort —
// nguồn dữ liệu profile hay bị Cloudflare chặn hơn link lẻ), chọn rồi tải,
// hoặc tải thẳng N video mới nhất không cần liệt kê.

import { useState } from 'react'
import {
  Alert,
  Button,
  Card,
  Empty,
  Flex,
  Input,
  InputNumber,
  message,
  Segmented,
  Space,
  Table,
  Tag,
  Typography,
} from 'antd'
import { DownloadOutlined, SearchOutlined, UserOutlined } from '@ant-design/icons'
import { api, fmtDuration, fmtNum, type FeedVideo, type Quality } from './api'

const { Text } = Typography

export default function ProfileTab({ onChanged }: { onChanged: () => void }) {
  const [uid, setUid] = useState('')
  const [quality, setQuality] = useState<Quality>('nowm')
  const [max, setMax] = useState(30)
  const [videos, setVideos] = useState<FeedVideo[]>([])
  const [cursor, setCursor] = useState('')
  const [hasMore, setHasMore] = useState(false)
  const [selected, setSelected] = useState<React.Key[]>([])
  const [err, setErr] = useState<{ msg: string; hint?: string } | null>(null)
  const [loading, setLoading] = useState(false)
  const [queueing, setQueueing] = useState(false)

  const load = async (append = false) => {
    if (!uid.trim()) return
    setLoading(true)
    setErr(null)
    try {
      const r = await api.profileFeed(uid, 30, append ? cursor : '')
      if (r.error || !r.feed) {
        setErr({ msg: r.error || 'Không lấy được danh sách video', hint: r.hint })
        if (!append) setVideos([])
        return
      }
      setVideos((old) => (append ? [...old, ...r.feed!.videos] : r.feed!.videos))
      setCursor(r.feed.cursor)
      setHasMore(r.feed.has_more)
    } finally {
      setLoading(false)
    }
  }

  const downloadSelected = async () => {
    const urls = videos.filter((v) => selected.includes(v.video_id)).map((v) => v.url)
    if (urls.length === 0) return
    setQueueing(true)
    try {
      const r = await api.batch(urls.join('\n'), quality)
      if (r.error) message.error(String(r.error))
      else message.success(`Đã xếp ${r.queued} video vào hàng đợi`)
      setSelected([])
      onChanged()
    } finally {
      setQueueing(false)
    }
  }

  const downloadNewest = async () => {
    if (!uid.trim()) return
    setQueueing(true)
    setErr(null)
    try {
      const r = await api.profileDownload(uid, max, quality)
      if (r.error) setErr({ msg: String(r.error), hint: r.hint })
      else
        message.success(
          `Tìm thấy ${r.found} video — xếp ${r.queued} vào hàng đợi${r.skipped_duplicates ? ` (bỏ qua ${r.skipped_duplicates} đã tải)` : ''}`,
        )
      onChanged()
    } finally {
      setQueueing(false)
    }
  }

  return (
    <Flex vertical gap={16}>
      <Card>
        <Flex vertical gap={12}>
          <Space wrap>
            <Input
              prefix={<UserOutlined />}
              placeholder="@tentaikhoan"
              value={uid}
              onChange={(e) => setUid(e.target.value)}
              onPressEnter={() => load(false)}
              style={{ width: 240 }}
            />
            <Segmented
              value={quality}
              onChange={(v) => setQuality(v as Quality)}
              options={[
                { value: 'nowm', label: 'Không logo' },
                { value: 'hd', label: 'HD' },
                { value: 'audio', label: 'Nhạc MP3' },
              ]}
            />
            <Button icon={<SearchOutlined />} onClick={() => load(false)} loading={loading} disabled={!uid.trim()}>
              Xem video
            </Button>
            <Space.Compact>
              <InputNumber min={1} max={200} value={max} onChange={(v) => setMax(v ?? 30)} style={{ width: 80 }} />
              <Button type="primary" icon={<DownloadOutlined />} onClick={downloadNewest} loading={queueing} disabled={!uid.trim()}>
                Tải video mới nhất
              </Button>
            </Space.Compact>
          </Space>
          {err && (
            <Alert
              type="warning"
              showIcon
              message={err.msg}
              description={err.hint}
            />
          )}
        </Flex>
      </Card>

      {videos.length > 0 ? (
        <Card
          size="small"
          title={`@${uid.replace(/^@/, '')} — ${videos.length} video`}
          extra={
            <Button
              type="primary"
              size="small"
              icon={<DownloadOutlined />}
              disabled={selected.length === 0}
              loading={queueing}
              onClick={downloadSelected}
            >
              Tải {selected.length} video đã chọn
            </Button>
          }
        >
          <Table
            rowKey="video_id"
            size="small"
            dataSource={videos}
            rowSelection={{ selectedRowKeys: selected, onChange: setSelected }}
            pagination={{ pageSize: 15, showSizeChanger: false }}
            columns={[
              {
                title: 'Video',
                key: 'v',
                render: (_: unknown, v: FeedVideo) => (
                  <Flex gap={10} align="center">
                    {v.cover ? (
                      <img src={v.cover} className="tdl-thumb" alt="" />
                    ) : (
                      <div className="tdl-thumb" />
                    )}
                    <div style={{ minWidth: 0 }}>
                      <div className="tdl-ellipsis" style={{ maxWidth: 420 }}>
                        <Text>{v.title || '(không caption)'}</Text>
                      </div>
                      <Space size={6}>
                        {v.is_images ? <Tag>Bộ ảnh</Tag> : <Tag>{fmtDuration(v.duration) || 'video'}</Tag>}
                        <Text type="secondary" style={{ fontSize: 12 }}>▶ {fmtNum(v.play_count)}</Text>
                        <a href={v.url} target="_blank" rel="noreferrer" style={{ fontSize: 12 }}>
                          mở
                        </a>
                      </Space>
                    </div>
                  </Flex>
                ),
              },
              {
                title: 'Đăng lúc',
                key: 't',
                width: 120,
                render: (_: unknown, v: FeedVideo) =>
                  v.create_time ? new Date(v.create_time * 1000).toLocaleDateString('vi-VN') : '—',
              },
            ]}
          />
          {hasMore && (
            <Button block onClick={() => load(true)} loading={loading} style={{ marginTop: 8 }}>
              Tải thêm trang nữa
            </Button>
          )}
        </Card>
      ) : (
        !err && (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="Nhập tên tài khoản rồi bấm Xem video, hoặc Tải video mới nhất để tải thẳng"
          />
        )
      )}
    </Flex>
  )
}
