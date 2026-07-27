import { useCallback, useEffect, useState } from 'react'
import { App, Button, Segmented, Space, Table, Tag, Typography } from 'antd'
import { api, type Draft } from '../api'

const STATUS_COLOR: Record<string, string> = {
  draft: 'default',
  approved: 'warning',
  sent: 'success',
  failed: 'error',
}

export function DraftsPage() {
  const { message } = App.useApp()
  const [filter, setFilter] = useState<string>('all')
  const [rows, setRows] = useState<Draft[]>([])
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setRows(await api.listDrafts(filter === 'all' ? undefined : filter))
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }, [filter, message])

  useEffect(() => {
    load()
  }, [load])

  const run = async (fn: () => Promise<unknown>, ok: string) => {
    try {
      await fn()
      message.success(ok)
      await load()
    } catch (e) {
      message.error(String(e))
    }
  }

  const columns = [
    { title: 'Nội dung', dataIndex: 'body' },
    {
      title: 'Loại',
      dataIndex: 'kind',
      width: 150,
      render: (k: string, d: Draft) => (
        <span>
          {k}
          {d.target && <div style={{ fontSize: 12, opacity: 0.6 }}>{d.target}</div>}
        </span>
      ),
    },
    {
      title: 'Trạng thái',
      dataIndex: 'status',
      width: 110,
      render: (s: string) => <Tag color={STATUS_COLOR[s] || 'default'}>{s}</Tag>,
    },
    {
      title: '',
      width: 120,
      render: (_: unknown, d: Draft) => (
        <Space>
          {d.status === 'draft' && (
            <Button size="small" onClick={() => run(() => api.approveDraft(d.id), 'Đã duyệt')}>
              Duyệt
            </Button>
          )}
          {d.status === 'approved' && (
            <Button size="small" type="primary" onClick={() => run(() => api.sendDraft(d.id), 'Đã gửi')}>
              Gửi
            </Button>
          )}
        </Space>
      ),
    },
  ]

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Typography.Title level={4} style={{ margin: 0 }}>
        Bản nháp (draft → duyệt → gửi)
      </Typography.Title>
      <Segmented
        value={filter}
        onChange={(v) => setFilter(v as string)}
        options={[
          { label: 'Tất cả', value: 'all' },
          { label: 'Nháp', value: 'draft' },
          { label: 'Đã duyệt', value: 'approved' },
          { label: 'Đã gửi', value: 'sent' },
          { label: 'Lỗi', value: 'failed' },
        ]}
      />
      <Table
        rowKey="id"
        size="middle"
        columns={columns}
        dataSource={rows}
        loading={loading}
        pagination={{ pageSize: 15, showSizeChanger: true, showTotal: (t) => `${t} bản nháp` }}
        locale={{ emptyText: 'Chưa có bản nháp nào.' }}
      />
    </Space>
  )
}
