// Tab Tải xuống: dán 1 hay nhiều link → phân tích (xem trước) hoặc tải ngay.
// Bên dưới là hàng đợi trực tiếp: job đang tải hiện % theo bytes, hủy được;
// job vừa xong / lỗi hiện gọn để thao tác nhanh (mở file, tải lại).

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Alert,
  Avatar,
  Button,
  Card,
  Descriptions,
  Empty,
  Flex,
  Input,
  List,
  message,
  Popconfirm,
  Progress,
  Segmented,
  Space,
  Tag,
  Typography,
} from 'antd'
import {
  CloseOutlined,
  DownloadOutlined,
  FolderOpenOutlined,
  PictureOutlined,
  ReloadOutlined,
  SearchOutlined,
  SoundOutlined,
} from '@ant-design/icons'
import {
  api,
  countLinks,
  fmtBytes,
  fmtDuration,
  fmtNum,
  KIND_LABEL,
  QUALITY_LABEL,
  type DownloadRow,
  type Meta,
  type Quality,
} from './api'

const { Text, Paragraph } = Typography

const STATUS_COLOR: Record<string, string> = {
  queued: 'default',
  resolving: 'processing',
  downloading: 'processing',
  done: 'success',
  error: 'error',
  canceled: 'warning',
}
const STATUS_LABEL: Record<string, string> = {
  queued: 'Chờ tải',
  resolving: 'Đang phân giải',
  downloading: 'Đang tải',
  done: 'Xong',
  error: 'Lỗi',
  canceled: 'Đã hủy',
}

export function Thumb({ row }: { row: { id: number; cover_url: string } }) {
  const [broken, setBroken] = useState(false)
  if (broken) {
    return (
      <div className="tdl-thumb" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <PictureOutlined style={{ color: 'var(--tdl-muted)' }} />
      </div>
    )
  }
  return (
    <img
      className="tdl-thumb"
      src={`/api/downloads/${row.id}/thumb`}
      onError={(e) => {
        // Thumb chưa kịp lưu → thử cover CDN (còn hạn ngay sau khi phân giải).
        const img = e.currentTarget
        if (row.cover_url && img.src.includes('/thumb')) img.src = row.cover_url
        else setBroken(true)
      }}
      alt=""
    />
  )
}

/** Một dòng trong hàng đợi / danh sách gần đây. */
export function JobRow({
  row,
  onChanged,
}: {
  row: DownloadRow
  onChanged: () => void
}) {
  const running = row.status === 'downloading' || row.status === 'resolving'
  const pct =
    row.total_bytes > 0 ? Math.min(100, Math.round((row.progress_bytes / row.total_bytes) * 100)) : 0
  const act = async (fn: () => Promise<any>, okMsg?: string) => {
    const r = await fn()
    if (r?.error) message.error(String(r.error))
    else if (okMsg) message.success(okMsg)
    onChanged()
  }
  return (
    <List.Item
      key={row.id}
      actions={[
        ...(running || row.status === 'queued'
          ? [
              <Popconfirm key="c" title="Hủy tải job này?" onConfirm={() => act(() => api.cancel(row.id))}>
                <Button size="small" danger icon={<CloseOutlined />}>Hủy</Button>
              </Popconfirm>,
            ]
          : []),
        ...(row.status === 'done'
          ? [
              <Button key="o" size="small" icon={<FolderOpenOutlined />} onClick={() => act(() => api.open(row.id, true))}>
                Mở
              </Button>,
            ]
          : []),
        ...(row.status === 'error' || row.status === 'canceled'
          ? [
              <Button key="r" size="small" icon={<ReloadOutlined />} onClick={() => act(() => api.retry(row.id), 'Đã xếp tải lại')}>
                Tải lại
              </Button>,
            ]
          : []),
      ]}
    >
      <Flex gap={12} align="center" style={{ width: '100%' }}>
        <Thumb row={row} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="tdl-ellipsis">
            <Text strong>{row.title || row.input_url}</Text>
          </div>
          <Space size={6} wrap style={{ marginTop: 4 }}>
            {row.author_id && <Text type="secondary">@{row.author_id}</Text>}
            <Tag>{QUALITY_LABEL[row.quality] ?? row.quality}</Tag>
            {row.kind && <Tag>{KIND_LABEL[row.kind] ?? row.kind}</Tag>}
            <Tag color={STATUS_COLOR[row.status]}>{STATUS_LABEL[row.status] ?? row.status}</Tag>
            {row.total_bytes > 0 && <Text type="secondary">{fmtBytes(row.total_bytes)}</Text>}
          </Space>
          {running && (
            <Progress
              percent={pct}
              size="small"
              status="active"
              format={() => (row.total_bytes > 0 ? `${pct}%` : fmtBytes(row.progress_bytes))}
            />
          )}
          {row.status === 'error' && (
            <Paragraph type="danger" style={{ margin: '4px 0 0', fontSize: 12 }} ellipsis={{ rows: 2 }}>
              {row.error}
            </Paragraph>
          )}
        </div>
      </Flex>
    </List.Item>
  )
}

/** Poll danh sách job: nhanh khi có job chạy, thưa khi rảnh. */
export function useJobs(filter: { status?: string }, deps: unknown[] = []) {
  const [rows, setRows] = useState<DownloadRow[]>([])
  const busy = rows.some((r) => ['queued', 'resolving', 'downloading'].includes(r.status))
  const refresh = useCallback(() => {
    api
      .list({ ...filter, limit: 100 })
      .then((r) => setRows(r.downloads ?? []))
      .catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
  useEffect(() => {
    refresh()
    const t = setInterval(refresh, busy ? 1200 : 5000)
    return () => clearInterval(t)
  }, [refresh, busy])
  return { rows, refresh }
}

export default function DownloadTab({ onChanged }: { onChanged: () => void }) {
  const [text, setText] = useState('')
  const [quality, setQuality] = useState<Quality>('nowm')
  const [resolving, setResolving] = useState(false)
  const [queueing, setQueueing] = useState(false)
  const [preview, setPreview] = useState<{ url: string; meta: Meta } | null>(null)
  const [resolveErr, setResolveErr] = useState('')
  const defaultQ = useRef(false)

  useEffect(() => {
    if (defaultQ.current) return
    defaultQ.current = true
    api.status().then((s) => setQuality((s.default_quality as Quality) || 'nowm')).catch(() => {})
  }, [])

  const links = countLinks(text)
  const { rows, refresh } = useJobs({})
  const activeRows = rows.filter((r) => ['queued', 'resolving', 'downloading'].includes(r.status))
  const recentRows = rows.filter((r) => !['queued', 'resolving', 'downloading'].includes(r.status)).slice(0, 6)

  const changed = () => {
    refresh()
    onChanged()
  }

  const analyze = async () => {
    setResolving(true)
    setResolveErr('')
    setPreview(null)
    try {
      const r = await api.resolve(text)
      if (r.error || !r.meta) setResolveErr(r.error || 'Không phân giải được link')
      else setPreview({ url: r.url!, meta: r.meta })
    } finally {
      setResolving(false)
    }
  }

  const enqueue = async (q: Quality, url?: string, meta?: Meta) => {
    setQueueing(true)
    try {
      const r =
        !url && links > 1
          ? await api.batch(text, q)
          : await api.download(url ?? text, q, false, meta)
      if (r.error) message.error(String(r.error))
      else if (r.duplicate) message.info(String(r.message))
      else if (r.queued !== undefined)
        message.success(
          `Đã xếp ${r.queued} link vào hàng đợi${r.skipped_duplicates ? ` (bỏ qua ${r.skipped_duplicates} đã tải)` : ''}`,
        )
      else message.success('Đã xếp vào hàng đợi')
      changed()
    } finally {
      setQueueing(false)
    }
  }

  const m = preview?.meta
  const sizeFor = (q: Quality) =>
    !m ? '' : q === 'hd' ? fmtBytes(m.hd_size) : q === 'wm' ? fmtBytes(m.wm_size) : fmtBytes(m.size)

  return (
    <Flex vertical gap={16}>
      <Card>
        <Flex vertical gap={12}>
          <Input.TextArea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={
              'Dán link TikTok vào đây — một hay nhiều link đều được:\nhttps://www.tiktok.com/@user/video/… · https://vm.tiktok.com/… · link lẫn trong chữ cũng nhận'
            }
            autoSize={{ minRows: 3, maxRows: 8 }}
            onPressEnter={(e) => {
              if (!e.shiftKey && links === 1) {
                e.preventDefault()
                analyze()
              }
            }}
          />
          <Flex justify="space-between" align="center" wrap gap={8}>
            <Space wrap>
              <Segmented
                value={quality}
                onChange={(v) => setQuality(v as Quality)}
                options={[
                  { value: 'nowm', label: 'Không logo' },
                  { value: 'hd', label: 'HD' },
                  { value: 'wm', label: 'Có logo' },
                  { value: 'audio', label: 'Nhạc MP3' },
                ]}
              />
              {links > 0 && <Tag color="blue">{links} link</Tag>}
            </Space>
            <Space>
              <Button icon={<SearchOutlined />} onClick={analyze} loading={resolving} disabled={links === 0}>
                Phân tích
              </Button>
              <Button
                type="primary"
                icon={<DownloadOutlined />}
                onClick={() => enqueue(quality)}
                loading={queueing}
                disabled={links === 0}
              >
                {links > 1 ? `Tải ${links} link` : 'Tải xuống'}
              </Button>
            </Space>
          </Flex>
          {resolveErr && <Alert type="error" showIcon message={resolveErr} />}
        </Flex>
      </Card>

      {m && preview && (
        <Card
          title={
            <Space>
              <Avatar src={m.author_avatar} size="small" />
              <Text strong>{m.author_name || `@${m.author_id}`}</Text>
              <Text type="secondary">@{m.author_id}</Text>
            </Space>
          }
          extra={
            <Button size="small" type="text" icon={<CloseOutlined />} onClick={() => setPreview(null)} />
          }
        >
          <Flex gap={16} wrap>
            {m.cover_url && (
              <img
                src={m.cover_url}
                alt=""
                style={{ width: 140, maxHeight: 200, objectFit: 'cover', borderRadius: 10 }}
              />
            )}
            <div style={{ flex: 1, minWidth: 260 }}>
              <Paragraph style={{ marginBottom: 8 }} ellipsis={{ rows: 3, expandable: true, symbol: 'xem thêm' }}>
                {m.title || '(không có caption)'}
              </Paragraph>
              <Descriptions
                size="small"
                column={{ xs: 1, sm: 2 }}
                items={[
                  {
                    key: 'k',
                    label: 'Loại',
                    children:
                      m.kind === 'images' ? `Bộ ảnh ${m.images.length} tấm` : `Video ${fmtDuration(m.duration)}`,
                  },
                  {
                    key: 's',
                    label: 'Tương tác',
                    children: `▶ ${fmtNum(m.stats.play_count)} · ♥ ${fmtNum(m.stats.digg_count)} · 💬 ${fmtNum(m.stats.comment_count)}`,
                  },
                  { key: 'm', label: 'Nhạc nền', children: m.music_title || '—' },
                  { key: 'r', label: 'Khu vực', children: m.stats.region || '—' },
                ]}
              />
              <Space wrap style={{ marginTop: 12 }}>
                {m.kind === 'images' ? (
                  <Button
                    type="primary"
                    icon={<PictureOutlined />}
                    onClick={() => enqueue('nowm', preview.url, m)}
                    loading={queueing}
                  >
                    Tải bộ ảnh
                  </Button>
                ) : (
                  <>
                    <Button type="primary" icon={<DownloadOutlined />} onClick={() => enqueue('nowm', preview.url, m)}>
                      Không logo {sizeFor('nowm') !== '—' && `(${sizeFor('nowm')})`}
                    </Button>
                    {m.hdplay && (
                      <Button icon={<DownloadOutlined />} onClick={() => enqueue('hd', preview.url, m)}>
                        HD {sizeFor('hd') !== '—' && `(${sizeFor('hd')})`}
                      </Button>
                    )}
                    {m.wmplay && (
                      <Button icon={<DownloadOutlined />} onClick={() => enqueue('wm', preview.url, m)}>
                        Có logo {sizeFor('wm') !== '—' && `(${sizeFor('wm')})`}
                      </Button>
                    )}
                  </>
                )}
                {m.music_url && (
                  <Button icon={<SoundOutlined />} onClick={() => enqueue('audio', preview.url, m)}>
                    Nhạc MP3
                  </Button>
                )}
              </Space>
            </div>
          </Flex>
        </Card>
      )}

      <Card title={`Hàng đợi ${activeRows.length > 0 ? `(${activeRows.length})` : ''}`} size="small">
        {activeRows.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Không có job nào đang chạy" />
        ) : (
          <List
            dataSource={activeRows}
            renderItem={(row) => <JobRow row={row} onChanged={changed} />}
            rowKey="id"
          />
        )}
      </Card>

      {recentRows.length > 0 && (
        <Card title="Vừa xong" size="small">
          <List dataSource={recentRows} renderItem={(row) => <JobRow row={row} onChanged={changed} />} rowKey="id" />
        </Card>
      )}
    </Flex>
  )
}
