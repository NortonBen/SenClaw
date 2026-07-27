import { useCallback, useEffect, useState } from 'react'
import { Button, Card, Table, Tag, Typography } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { api, type AlertItem } from '../api'
import { levelColor, POLL_MS } from '../ui'

const { Text } = Typography

export default function Alerts() {
  const [alerts, setAlerts] = useState<AlertItem[]>([])
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(() => {
    api
      .alerts(100)
      .then(setAlerts)
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, POLL_MS * 2)
    return () => clearInterval(t)
  }, [refresh])

  const columns = [
    {
      title: 'Mức',
      dataIndex: 'level',
      width: 120,
      filters: [...new Set(alerts.map((a) => a.level))].map((l) => ({ text: l, value: l })),
      onFilter: (v: unknown, r: AlertItem) => r.level === v,
      render: (level: string) => (
        <Tag color={levelColor(level)} style={{ textTransform: 'uppercase' }}>
          {level}
        </Tag>
      ),
    },
    {
      title: 'Thiết bị',
      dataIndex: 'device_name',
      width: 220,
      render: (name: string, r: AlertItem) => name || r.device_id,
    },
    { title: 'Nội dung', dataIndex: 'message' },
    {
      title: 'Thời gian',
      dataIndex: 'ts',
      width: 220,
      render: (ts: string) => <Text type="secondary">{ts}</Text>,
    },
  ]

  return (
    <Card
      title="Cảnh báo (7 ngày gần nhất)"
      size="small"
      extra={
        <Button size="small" icon={<ReloadOutlined />} onClick={refresh}>
          Làm mới
        </Button>
      }
      styles={{ body: { padding: 0 } }}
    >
      <Table
        rowKey="id"
        size="small"
        loading={loading}
        columns={columns}
        dataSource={alerts}
        pagination={{ pageSize: 20, showSizeChanger: false }}
      />
    </Card>
  )
}
