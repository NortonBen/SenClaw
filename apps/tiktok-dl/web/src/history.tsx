// Tab Lịch sử: toàn bộ bản ghi tải với tìm kiếm FTS (gõ không dấu vẫn khớp),
// lọc trạng thái/loại, và thao tác từng dòng (mở file, tải qua trình duyệt,
// tải lại, xoá ± file). Dọn hàng loạt qua nút "Dọn lịch sử".

import { useCallback, useEffect, useState } from 'react'
import {
  Button,
  Dropdown,
  Flex,
  Input,
  message,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import {
  ClearOutlined,
  CloseOutlined,
  DeleteOutlined,
  DownloadOutlined,
  FolderOpenOutlined,
  LinkOutlined,
  ReloadOutlined,
} from '@ant-design/icons'
import { api, fmtBytes, KIND_LABEL, QUALITY_LABEL, type DownloadRow } from './api'
import { Thumb } from './download'

const { Text } = Typography

const STATUS_TAG: Record<string, { color: string; label: string }> = {
  queued: { color: 'default', label: 'Chờ' },
  resolving: { color: 'processing', label: 'Phân giải' },
  downloading: { color: 'processing', label: 'Đang tải' },
  done: { color: 'success', label: 'Xong' },
  error: { color: 'error', label: 'Lỗi' },
  canceled: { color: 'warning', label: 'Đã hủy' },
}

export default function HistoryTab({ onChanged }: { onChanged: () => void }) {
  const [rows, setRows] = useState<DownloadRow[]>([])
  const [loading, setLoading] = useState(false)
  const [q, setQ] = useState('')
  const [status, setStatus] = useState<string>('')
  const [kind, setKind] = useState<string>('')

  const refresh = useCallback(() => {
    setLoading(true)
    api
      .list({ q, status, kind, limit: 400 })
      .then((r) => setRows(r.downloads ?? []))
      .finally(() => setLoading(false))
  }, [q, status, kind])

  useEffect(() => {
    refresh()
  }, [refresh])

  const changed = () => {
    refresh()
    onChanged()
  }

  const act = async (fn: () => Promise<any>, okMsg?: string) => {
    const r = await fn()
    if (r?.error) message.error(String(r.error))
    else if (okMsg) message.success(okMsg)
    changed()
  }

  const clearMenu = (withFiles: boolean) => ({
    items: [
      { key: 'all', label: 'Mọi bản ghi đã kết thúc' },
      { key: 'error', label: 'Chỉ bản ghi lỗi' },
      { key: 'canceled', label: 'Chỉ bản ghi đã hủy' },
    ],
    onClick: ({ key }: { key: string }) => {
      const label = key === 'all' ? 'đã kết thúc' : key === 'error' ? 'lỗi' : 'đã hủy'
      Modal.confirm({
        title: withFiles ? `Xoá bản ghi ${label} VÀ FILE trên đĩa?` : `Dọn bản ghi ${label}?`,
        content: withFiles
          ? 'File đã tải sẽ bị xoá khỏi ổ đĩa — không hoàn tác được.'
          : 'Chỉ xoá bản ghi trong lịch sử, file đã tải giữ nguyên.',
        okText: 'Xoá',
        okButtonProps: { danger: withFiles },
        cancelText: 'Thôi',
        onOk: () =>
          api.clear(key === 'all' ? undefined : key, withFiles).then((r) => {
            if (r?.error) message.error(String(r.error))
            else message.success(`Đã dọn ${r.cleared} bản ghi${withFiles ? `, xoá ${r.removed_files} file` : ''}`)
            changed()
          }),
      })
    },
  })

  const columns = [
    {
      title: 'Nội dung',
      key: 'title',
      render: (_: unknown, r: DownloadRow) => (
        <Flex gap={10} align="center">
          <Thumb row={r} />
          <div style={{ minWidth: 0 }}>
            <div className="tdl-ellipsis" style={{ maxWidth: 380 }}>
              <Text strong>{r.title || r.input_url}</Text>
            </div>
            <Space size={6} wrap>
              {r.author_id && <Text type="secondary" style={{ fontSize: 12 }}>@{r.author_id}</Text>}
              <a href={r.input_url} target="_blank" rel="noreferrer" style={{ fontSize: 12 }}>
                <LinkOutlined /> mở TikTok
              </a>
            </Space>
          </div>
        </Flex>
      ),
    },
    {
      title: 'Loại',
      key: 'kind',
      width: 130,
      render: (_: unknown, r: DownloadRow) => (
        <Space direction="vertical" size={2}>
          <Tag>{KIND_LABEL[r.kind] ?? r.kind ?? '—'}</Tag>
          <Tag color="blue">{QUALITY_LABEL[r.quality] ?? r.quality}</Tag>
        </Space>
      ),
    },
    {
      title: 'Dung lượng',
      key: 'size',
      width: 100,
      render: (_: unknown, r: DownloadRow) => <Text>{fmtBytes(r.total_bytes)}</Text>,
    },
    {
      title: 'Trạng thái',
      key: 'status',
      width: 130,
      render: (_: unknown, r: DownloadRow) => {
        const t = STATUS_TAG[r.status] ?? { color: 'default', label: r.status }
        const pct = r.total_bytes > 0 ? Math.round((r.progress_bytes / r.total_bytes) * 100) : 0
        return (
          <Space direction="vertical" size={2} style={{ width: '100%' }}>
            <Tooltip title={r.error || undefined}>
              <Tag color={t.color}>{t.label}</Tag>
            </Tooltip>
            {(r.status === 'downloading' || r.status === 'resolving') && (
              <Progress percent={pct} size="small" status="active" showInfo={false} />
            )}
          </Space>
        )
      },
    },
    {
      title: 'Lúc',
      key: 'at',
      width: 105,
      render: (_: unknown, r: DownloadRow) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {(r.finished_at || r.created_at || '').replace('T', ' ').slice(5, 16)}
        </Text>
      ),
    },
    {
      title: '',
      key: 'actions',
      width: 170,
      render: (_: unknown, r: DownloadRow) => (
        <Space size={4} wrap>
          {['queued', 'resolving', 'downloading'].includes(r.status) && (
            <Popconfirm title="Hủy job này?" onConfirm={() => act(() => api.cancel(r.id))}>
              <Button size="small" danger icon={<CloseOutlined />} />
            </Popconfirm>
          )}
          {r.status === 'done' && r.files.length > 0 && (
            <>
              <Tooltip title="Mở trong Finder">
                <Button size="small" icon={<FolderOpenOutlined />} onClick={() => act(() => api.open(r.id, true))} />
              </Tooltip>
              <Tooltip title="Tải về qua trình duyệt">
                <Button
                  size="small"
                  icon={<DownloadOutlined />}
                  onClick={() => window.open(`/api/downloads/${r.id}/file?i=0`, '_blank')}
                />
              </Tooltip>
            </>
          )}
          {['error', 'canceled', 'done'].includes(r.status) && (
            <Tooltip title="Tải lại">
              <Button size="small" icon={<ReloadOutlined />} onClick={() => act(() => api.retry(r.id), 'Đã xếp tải lại')} />
            </Tooltip>
          )}
          {!['queued', 'resolving', 'downloading'].includes(r.status) && (
            <Dropdown
              menu={{
                items: [
                  { key: 'rec', label: 'Xoá bản ghi (giữ file)' },
                  { key: 'file', label: 'Xoá bản ghi + file', danger: true },
                ],
                onClick: ({ key }) =>
                  act(() => api.delete(r.id, key === 'file'), key === 'file' ? 'Đã xoá bản ghi và file' : 'Đã xoá bản ghi'),
              }}
              trigger={['click']}
            >
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Dropdown>
          )}
        </Space>
      ),
    },
  ]

  return (
    <Flex vertical gap={12}>
      <Flex justify="space-between" wrap gap={8}>
        <Space wrap>
          <Input.Search
            placeholder="Tìm caption / tác giả / link — gõ không dấu vẫn khớp"
            allowClear
            style={{ width: 320 }}
            onSearch={setQ}
          />
          <Select
            placeholder="Trạng thái"
            allowClear
            style={{ width: 130 }}
            onChange={(v) => setStatus(v ?? '')}
            options={[
              { value: 'active', label: 'Đang chạy' },
              { value: 'done', label: 'Xong' },
              { value: 'error', label: 'Lỗi' },
              { value: 'canceled', label: 'Đã hủy' },
            ]}
          />
          <Select
            placeholder="Loại"
            allowClear
            style={{ width: 120 }}
            onChange={(v) => setKind(v ?? '')}
            options={Object.entries(KIND_LABEL).map(([value, label]) => ({ value, label }))}
          />
        </Space>
        <Space>
          <Dropdown menu={clearMenu(false)} trigger={['click']}>
            <Button icon={<ClearOutlined />}>Dọn lịch sử</Button>
          </Dropdown>
          <Dropdown menu={clearMenu(true)} trigger={['click']}>
            <Button danger icon={<DeleteOutlined />}>Xoá cả file…</Button>
          </Dropdown>
        </Space>
      </Flex>

      <Table
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={rows}
        columns={columns}
        pagination={{ pageSize: 20, showSizeChanger: false, showTotal: (t) => `${t} bản ghi` }}
      />
    </Flex>
  )
}
