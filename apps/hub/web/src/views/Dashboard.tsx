import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert,
  Badge,
  Button,
  Card,
  Col,
  Empty,
  Flex,
  Input,
  Row,
  Statistic,
  Table,
  Tag,
  Typography,
} from 'antd'
import {
  AlertOutlined,
  CheckCircleOutlined,
  DatabaseOutlined,
  DisconnectOutlined,
  ReloadOutlined,
} from '@ant-design/icons'
import { api, type AlertItem, type Device } from '../api'
import { AttrTags, levelColor, POLL_MS } from '../ui'

const { Text } = Typography

export default function Dashboard({ onOpen }: { onOpen: (id: string) => void }) {
  const [devices, setDevices] = useState<Device[] | null>(null)
  const [alerts, setAlerts] = useState<AlertItem[]>([])
  const [q, setQ] = useState('')
  const [err, setErr] = useState('')

  const refresh = useCallback(() => {
    api
      .devices()
      .then((d) => {
        setDevices(d)
        setErr('')
      })
      .catch((e) => setErr(e.message))
    api.alerts().then(setAlerts).catch(() => {})
  }, [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, POLL_MS)
    return () => clearInterval(t)
  }, [refresh])

  const filtered = useMemo(
    () =>
      (devices ?? []).filter(
        (d) =>
          !q ||
          d.name.toLowerCase().includes(q.toLowerCase()) ||
          d.id.toLowerCase().includes(q.toLowerCase()),
      ),
    [devices, q],
  )

  const online = (devices ?? []).filter((d) => d.online).length
  const total = devices?.length ?? 0

  const alertColumns = [
    {
      title: 'Mức',
      dataIndex: 'level',
      width: 110,
      render: (level: string) => (
        <Tag color={levelColor(level)} style={{ textTransform: 'uppercase' }}>
          {level}
        </Tag>
      ),
    },
    {
      title: 'Thiết bị',
      dataIndex: 'device_name',
      render: (name: string, r: AlertItem) => name || r.device_id,
    },
    { title: 'Nội dung', dataIndex: 'message' },
    {
      title: 'Thời gian',
      dataIndex: 'ts',
      width: 210,
      render: (ts: string) => <Text type="secondary">{ts}</Text>,
    },
  ]

  return (
    <div>
      {err && <Alert type="error" message={err} showIcon style={{ marginBottom: 16 }} />}

      <Row gutter={[14, 14]} style={{ marginBottom: 18 }}>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Tổng thiết bị" value={total} prefix={<DatabaseOutlined />} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic
              title="Online"
              value={online}
              valueStyle={{ color: '#34d399' }}
              prefix={<CheckCircleOutlined />}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic
              title="Offline"
              value={total - online}
              valueStyle={{ color: total - online > 0 ? '#f87171' : undefined }}
              prefix={<DisconnectOutlined />}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic
              title="Cảnh báo (7 ngày)"
              value={alerts.length}
              valueStyle={{ color: alerts.length > 0 ? '#fbbf24' : undefined }}
              prefix={<AlertOutlined />}
            />
          </Card>
        </Col>
      </Row>

      <Flex gap={12} align="center" wrap style={{ marginBottom: 16 }}>
        <Input.Search
          placeholder="Tìm thiết bị…"
          allowClear
          value={q}
          onChange={(e) => setQ(e.target.value)}
          style={{ maxWidth: 300 }}
        />
        <Button icon={<ReloadOutlined />} onClick={refresh}>
          Làm mới
        </Button>
      </Flex>

      {devices === null ? (
        <Card loading />
      ) : filtered.length === 0 ? (
        <Empty description="Không có thiết bị nào." />
      ) : (
        <Row gutter={[14, 14]}>
          {filtered.map((d) => (
            <Col key={d.id} xs={24} sm={12} lg={8} xl={6}>
              <Card
                className="device-card"
                hoverable
                size="small"
                onClick={() => onOpen(d.id)}
                title={
                  <Flex align="center" justify="space-between">
                    <span>
                      <Badge status={d.online ? 'success' : 'default'} /> {d.name}
                    </span>
                    <Tag color={d.online ? 'green' : 'default'}>
                      {d.online ? 'ONLINE' : 'OFFLINE'}
                    </Tag>
                  </Flex>
                }
              >
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {d.model || 'không rõ model'}
                  {d.last_seen ? ` · ${d.last_seen}` : ''}
                </Text>
                <AttrTags attributes={d.attributes} max={4} />
              </Card>
            </Col>
          ))}
        </Row>
      )}

      {alerts.length > 0 && (
        <Card
          title="Cảnh báo gần đây"
          size="small"
          style={{ marginTop: 20 }}
          styles={{ body: { padding: 0 } }}
        >
          <Table
            rowKey="id"
            size="small"
            columns={alertColumns}
            dataSource={alerts.slice(0, 8)}
            pagination={false}
          />
        </Card>
      )}
    </div>
  )
}
