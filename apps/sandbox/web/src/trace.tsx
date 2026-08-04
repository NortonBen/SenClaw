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
import { useT } from './i18n'

type Filter = 'all' | 'file' | 'proc' | 'net'

type Words = ReturnType<typeof useT>

function kindTag(kind: string, t: Words): { text: string; color: string } {
  switch (kind) {
    case 'file.read':
      return { text: t.evFileRead, color: 'blue' }
    case 'file.write':
      return { text: t.evFileWrite, color: 'orange' }
    case 'proc.spawn':
      return { text: t.evProcSpawn, color: 'purple' }
    case 'net.connect':
      return { text: t.evNetConnect, color: 'red' }
    case 'net.dns':
      return { text: t.evNetDns, color: 'magenta' }
    case 'trace.truncated':
      return { text: t.evTruncated, color: 'gold' }
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
  const t = useT()
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
      message.success(on ? t.traceOn : t.traceOff)
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
      title: t.colTime,
      dataIndex: 'tsMs',
      width: 110,
      render: (v: number) =>
        new Date(v).toLocaleTimeString('vi-VN', { hour12: false }) +
        '.' +
        String(v % 1000).padStart(3, '0'),
    },
    {
      title: t.colKind,
      dataIndex: 'kind',
      width: 130,
      render: (v: string) => {
        const k = kindTag(v, t)
        return <Tag color={k.color}>{k.text}</Tag>
      },
    },
    {
      title: t.colTarget,
      dataIndex: 'target',
      render: (v: string) => (
        <Typography.Text className="sbx-mono" ellipsis={{ tooltip: v }} style={{ fontSize: 12 }}>
          {v}
        </Typography.Text>
      ),
    },
    {
      title: t.colDetail,
      dataIndex: 'detail',
      width: 200,
      render: (v: string) => (
        <Typography.Text type="secondary" ellipsis={{ tooltip: v }} style={{ fontSize: 12 }}>
          {v}
        </Typography.Text>
      ),
    },
    {
      title: t.colSource,
      dataIndex: 'source',
      width: 90,
      render: (v: string) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {v === 'diff' ? t.srcDiff : v}
        </Typography.Text>
      ),
    },
  ]

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={14}>
      <Space wrap size={12}>
        <Space size={8}>
          <Switch checked={sandbox.traceEnabled} onChange={toggle} />
          <Typography.Text>{t.traceToggle}</Typography.Text>
        </Space>
        <Segmented
          value={filter}
          onChange={(v) => setFilter(v as Filter)}
          options={[
            { label: t.filterAll, value: 'all' },
            { label: t.filterFile, value: 'file' },
            { label: t.filterProc, value: 'proc' },
            { label: t.filterNet, value: 'net' },
          ]}
        />
        <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>
          {t.reload}
        </Button>
        <Popconfirm
          title={t.clearLogConfirm}
          okText={t.delete}
          cancelText={t.cancel}
          onConfirm={() => void clear()}
        >
          <Button size="small" danger icon={<DeleteOutlined />}>
            {t.clearLog}
          </Button>
        </Popconfirm>
      </Space>

      {/* Stated up front, not in a footnote: someone reading a clean timeline
          should not conclude the code was proven harmless. */}
      <Alert
        type="warning"
        showIcon
        message={t.traceWarnTitle}
        description={t.traceWarnBody}
      />

      {!sandbox.traceEnabled && events.length === 0 ? (
        <Empty
          description={t.traceOffEmpty}
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      ) : events.length === 0 ? (
        <Empty
          description={t.traceOnEmpty}
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
