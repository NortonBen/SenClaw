import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Badge,
  Button,
  Card,
  Col,
  Empty,
  Flex,
  Form,
  Input,
  Row,
  Segmented,
  Space,
  Table,
  Typography,
} from 'antd'
import { ArrowLeftOutlined, PoweroffOutlined, ThunderboltOutlined } from '@ant-design/icons'
import { api, type Device, type TelemetryPoint } from '../api'
import { AttrTags, formatVal, POLL_MS, Sparkline } from '../ui'

const { Text, Title } = Typography

export default function DeviceDetail({ id, onBack }: { id: string; onBack: () => void }) {
  const [device, setDevice] = useState<Device | null>(null)
  const [points, setPoints] = useState<TelemetryPoint[]>([])
  const [field, setField] = useState('')
  const [err, setErr] = useState('')

  const refresh = useCallback(() => {
    api.device(id).then(setDevice).catch((e) => setErr(e.message))
    api
      .telemetry(id, field)
      .then((p) => {
        setPoints(p)
        setErr('')
      })
      .catch((e) => setErr(e.message))
  }, [id, field])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, POLL_MS)
    return () => clearInterval(t)
  }, [refresh])

  const fields = useMemo(() => [...new Set(points.map((p) => p.field))], [points])
  const activeField = field || fields[0] || ''
  const numeric = useMemo(
    () =>
      points
        .filter((p) => p.field === activeField && typeof p.value === 'number')
        .map((p) => p.value as number),
    [points, activeField],
  )

  const telemetryColumns = [
    {
      title: 'Thời gian',
      dataIndex: 'ts',
      width: 230,
      render: (ts: string) => <Text type="secondary">{ts}</Text>,
    },
    { title: 'Trường', dataIndex: 'field' },
    { title: 'Giá trị', dataIndex: 'value', render: (v: unknown) => formatVal(v) },
  ]

  return (
    <div>
      {err && <Alert type="error" message={err} showIcon style={{ marginBottom: 16 }} />}
      <Space size={12} style={{ marginBottom: 16 }} align="center" wrap>
        <Button icon={<ArrowLeftOutlined />} onClick={onBack} />
        <Badge status={device?.online ? 'success' : 'default'} />
        <Title level={4} style={{ margin: 0 }}>
          {device?.name ?? id}
        </Title>
        <Text type="secondary">
          {device?.model}
          {device?.last_seen ? ` · lần cuối: ${device.last_seen}` : ''}
        </Text>
      </Space>
      <Row gutter={[14, 14]}>
        <Col xs={24} lg={14}>
          <Card title="Telemetry" size="small">
            {fields.length > 0 && (
              <Segmented
                options={fields}
                value={activeField}
                onChange={(v) => setField(String(v))}
                style={{ marginBottom: 8 }}
              />
            )}
            {numeric.length > 1 && <Sparkline values={[...numeric].reverse()} />}
            {points.length === 0 ? (
              <Empty description="Chưa có dữ liệu telemetry." />
            ) : (
              <Table
                rowKey={(_, i) => String(i)}
                size="small"
                columns={telemetryColumns}
                dataSource={points.slice(0, 20)}
                pagination={false}
              />
            )}
          </Card>
        </Col>
        <Col xs={24} lg={10}>
          <Space direction="vertical" size={14} style={{ width: '100%' }}>
            <Card title="Thuộc tính" size="small">
              {Object.keys(device?.attributes ?? {}).length === 0 ? (
                <Text type="secondary">Không có thuộc tính.</Text>
              ) : (
                <AttrTags attributes={device?.attributes ?? {}} max={12} />
              )}
            </Card>
            <ControlPanel deviceId={id} online={!!device?.online} onSent={refresh} />
          </Space>
        </Col>
      </Row>
    </div>
  )
}

function ControlPanel({
  deviceId,
  online,
  onSent,
}: {
  deviceId: string
  online: boolean
  onSent: () => void
}) {
  const { message } = AntApp.useApp()
  const [form] = Form.useForm<{ command: string; params: string }>()
  const [busy, setBusy] = useState(false)

  const send = async (preset?: { command: string; params: string }) => {
    const values = preset ?? form.getFieldsValue()
    if (preset) form.setFieldsValue(preset)
    if (!values.command) {
      message.error('Nhập tên lệnh trước.')
      return
    }
    let parsed: Record<string, unknown>
    try {
      parsed = values.params.trim() ? JSON.parse(values.params) : {}
    } catch {
      message.error('Tham số không phải JSON hợp lệ.')
      return
    }
    setBusy(true)
    try {
      const r = await api.sendCommand(deviceId, values.command, parsed)
      if (r.ok) {
        message.success(r.detail || 'Đã gửi lệnh.')
      } else {
        message.error(r.detail || 'Gửi lệnh thất bại.')
      }
      onSent()
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card
      title={
        <Space>
          <ThunderboltOutlined /> Điều khiển
        </Space>
      }
      size="small"
    >
      {!online && (
        <Alert
          type="warning"
          showIcon
          message="Thiết bị đang offline — lệnh có thể không tới nơi."
          style={{ marginBottom: 12 }}
        />
      )}
      <Form
        form={form}
        layout="vertical"
        initialValues={{ command: 'sendMsgToDevice', params: '{"on": true}' }}
        onFinish={() => send()}
      >
        <Form.Item name="command" label="Lệnh">
          <Input />
        </Form.Item>
        <Form.Item name="params" label="Tham số (JSON)" className="mono-input">
          <Input className="mono-input" />
        </Form.Item>
        <Flex gap={8} wrap>
          <Button type="primary" htmlType="submit" loading={busy} icon={<ThunderboltOutlined />}>
            Gửi lệnh
          </Button>
          <Button
            icon={<PoweroffOutlined />}
            onClick={() => send({ command: 'sendMsgToDevice', params: '{"on": true}' })}
          >
            Bật
          </Button>
          <Button
            icon={<PoweroffOutlined />}
            onClick={() => send({ command: 'sendMsgToDevice', params: '{"on": false}' })}
          >
            Tắt
          </Button>
        </Flex>
      </Form>
    </Card>
  )
}
