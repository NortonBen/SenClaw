import { useEffect, useState } from 'react'
import { Alert, App as AntApp, Button, Card, Col, Descriptions, Form, Input, Row, Space } from 'antd'
import { CloudServerOutlined, SaveOutlined } from '@ant-design/icons'
import { api, type ConnStatus } from '../api'

export default function Settings({
  status,
  onStatus,
}: {
  status: ConnStatus | null
  onStatus: (s: ConnStatus) => void
}) {
  const { message } = AntApp.useApp()
  const [form] = Form.useForm<{ base_url: string; namespace: string }>()
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api
      .getSettings()
      .then((s) => form.setFieldsValue({ base_url: s.base_url, namespace: s.namespace }))
      .catch(() => {})
  }, [form])

  const save = async (values: { base_url: string; namespace: string }) => {
    setBusy(true)
    try {
      const st = await api.saveSettings({
        base_url: values.base_url,
        namespace: values.namespace ?? '',
      })
      onStatus(st)
      message.success('Đã lưu địa chỉ máy chủ. Đăng nhập lại để áp dụng.')
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Row gutter={[14, 14]}>
      <Col xs={24} lg={12}>
        <Card
          title={
            <Space>
              <CloudServerOutlined /> Máy chủ Dipper Hub
            </Space>
          }
          size="small"
        >
          <Alert
            type="info"
            showIcon
            message="Đổi địa chỉ máy chủ sẽ đăng xuất phiên hiện tại — đăng nhập lại sau khi lưu."
            style={{ marginBottom: 16 }}
          />
          <Form form={form} layout="vertical" onFinish={save} requiredMark={false}>
            <Form.Item
              name="base_url"
              label="Địa chỉ máy chủ (base URL)"
              rules={[{ required: true, message: 'Nhập URL máy chủ' }]}
            >
              <Input placeholder="http://localhost:8080" />
            </Form.Item>
            <Form.Item name="namespace" label="Namespace (tuỳ chọn)">
              <Input />
            </Form.Item>
            <Button type="primary" htmlType="submit" loading={busy} icon={<SaveOutlined />}>
              Lưu
            </Button>
          </Form>
        </Card>
      </Col>
      <Col xs={24} lg={12}>
        <Card title="Phiên hiện tại" size="small">
          <Descriptions
            column={1}
            size="small"
            items={[
              { label: 'Máy chủ', children: status?.base_url || '—' },
              { label: 'Tài khoản', children: status?.username || '—' },
              {
                label: 'Trạng thái',
                children: status?.connected ? 'Đã đăng nhập' : 'Chưa đăng nhập',
              },
            ]}
          />
        </Card>
      </Col>
    </Row>
  )
}
