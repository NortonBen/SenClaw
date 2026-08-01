import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Empty,
  Popconfirm,
  Segmented,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
} from 'antd'
import { DeleteOutlined, ReloadOutlined } from '@ant-design/icons'
import { api, type Sandbox, type TraceEvent } from './api'

type Filter = 'all' | 'file' | 'proc' | 'net'

function kindTag(kind: string): { text: string; color: string } {
  switch (kind) {
    case 'file.read':
      return { text: 'đọc file', color: 'blue' }
    case 'file.write':
      return { text: 'ghi file', color: 'orange' }
    case 'proc.spawn':
      return { text: 'tiến trình', color: 'purple' }
    case 'net.connect':
      return { text: 'kết nối', color: 'red' }
    case 'net.dns':
      return { text: 'tra tên miền', color: 'magenta' }
    case 'trace.truncated':
      return { text: 'bị cắt', color: 'gold' }
    default:
      return { text: kind, color: 'default' }
  }
}

/** Timeline of what the sandbox actually touched. */
export function TracePanel({
  sandbox,
  onChange,
}: {
  sandbox: Sandbox
  onChange: () => void
}) {
  const { message } = AntApp.useApp()
  const [events, setEvents] = useState<TraceEvent[]>([])
  const [filter, setFilter] = useState<Filter>('all')
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const r = await api.events(sandbox.id, filter === 'all' ? undefined : filter)
      setEvents(r.events)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [sandbox.id, filter, message])

  useEffect(() => {
    void load()
  }, [load])

  const toggle = async (on: boolean) => {
    try {
      await api.setTrace(sandbox.id, on)
      onChange()
      message.success(on ? 'Đã bật theo dõi — chạy lại mã để ghi nhận' : 'Đã tắt theo dõi')
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const clear = async () => {
    try {
      await api.clearEvents(sandbox.id)
      void load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const columns = [
    {
      title: 'Lúc',
      dataIndex: 'tsMs',
      width: 110,
      render: (v: number) =>
        new Date(v).toLocaleTimeString('vi-VN', { hour12: false }) +
        '.' +
        String(v % 1000).padStart(3, '0'),
    },
    {
      title: 'Loại',
      dataIndex: 'kind',
      width: 130,
      render: (v: string) => {
        const k = kindTag(v)
        return <Tag color={k.color}>{k.text}</Tag>
      },
    },
    {
      title: 'Đối tượng',
      dataIndex: 'target',
      render: (v: string) => (
        <Typography.Text className="sbx-mono" ellipsis={{ tooltip: v }} style={{ fontSize: 12 }}>
          {v}
        </Typography.Text>
      ),
    },
    {
      title: 'Chi tiết',
      dataIndex: 'detail',
      width: 200,
      render: (v: string) => (
        <Typography.Text type="secondary" ellipsis={{ tooltip: v }} style={{ fontSize: 12 }}>
          {v}
        </Typography.Text>
      ),
    },
    {
      title: 'Nguồn',
      dataIndex: 'source',
      width: 90,
      render: (v: string) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {v === 'diff' ? 'so sánh' : v}
        </Typography.Text>
      ),
    },
  ]

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={14}>
      <Space wrap size={12}>
        <Space size={8}>
          <Switch checked={sandbox.traceEnabled} onChange={toggle} />
          <Typography.Text>Theo dõi hoạt động</Typography.Text>
        </Space>
        <Segmented
          value={filter}
          onChange={(v) => setFilter(v as Filter)}
          options={[
            { label: 'Tất cả', value: 'all' },
            { label: 'File', value: 'file' },
            { label: 'Tiến trình', value: 'proc' },
            { label: 'Mạng', value: 'net' },
          ]}
        />
        <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>
          Tải lại
        </Button>
        <Popconfirm
          title="Xoá toàn bộ sự kiện đã ghi?"
          okText="Xoá"
          cancelText="Thôi"
          onConfirm={() => void clear()}
        >
          <Button size="small" danger icon={<DeleteOutlined />}>
            Xoá nhật ký
          </Button>
        </Popconfirm>
      </Space>

      {/* Stated up front, not in a footnote: someone reading a clean timeline
          should not conclude the code was proven harmless. */}
      <Alert
        type="warning"
        showIcon
        message="Đây là công cụ quan sát cho kiểm thử, KHÔNG phải bằng chứng an ninh"
        description="Hook theo dõi chạy bên trong sandbox, nhật ký cũng nằm trong thư mục sandbox — mã cố tình lẩn tránh thì né được. Thứ thật sự chặn được mã độc là bản thân sandbox (cách ly đọc, ghi, mạng), do nhân hệ điều hành cưỡng chế."
      />

      {!sandbox.traceEnabled && events.length === 0 ? (
        <Empty
          description="Theo dõi đang tắt. Bật lên rồi chạy lại mã để ghi nhận hoạt động."
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      ) : events.length === 0 ? (
        <Empty
          description="Chưa ghi nhận sự kiện nào — hãy chạy mã trong sandbox này."
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      ) : (
        <Table
          size="small"
          rowKey={(r) => `${r.tsMs}-${r.kind}-${r.target}-${r.pid}`}
          loading={loading}
          dataSource={events}
          columns={columns}
          pagination={{ pageSize: 25, showSizeChanger: false }}
          scroll={{ x: true }}
        />
      )}
    </Space>
  )
}
