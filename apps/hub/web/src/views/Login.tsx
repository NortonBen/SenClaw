import { useState } from 'react'
import { Alert, Button, Card, Flex, Form, Input, Modal, Space, Typography, theme } from 'antd'
import {
  ApiOutlined,
  CloudServerOutlined,
  LockOutlined,
  SettingOutlined,
  UserOutlined,
} from '@ant-design/icons'
import Logo from '../Logo'
import { api, type ConnStatus } from '../api'

const { Text, Title } = Typography

export default function Login({
  status,
  onStatus,
}: {
  status: ConnStatus | null
  onStatus: (s: ConnStatus) => void
}) {
  const { token } = theme.useToken()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [showServer, setShowServer] = useState(false)

  const login = async (values: { username: string; password: string }) => {
    setBusy(true)
    setErr('')
    try {
      const st = await api.login(values.username, values.password)
      onStatus(st)
      if (!st.connected) setErr(st.message)
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Flex
      align="center"
      justify="center"
      style={{
        minHeight: '100vh',
        background: `radial-gradient(1000px 500px at 20% -10%, ${token.colorPrimaryBg} 0%, ${token.colorBgLayout} 55%)`,
        padding: 20,
      }}
    >
      <Card style={{ width: 380, boxShadow: token.boxShadowSecondary }}>
        <Flex vertical align="center" gap={4} style={{ marginBottom: 20 }}>
          <Logo size={64} />
          <Title level={3} style={{ margin: '8px 0 0' }}>
            Device Hub
          </Title>
          <Text type="secondary">Bảng điều khiển thiết bị · Dipper IoT Hub</Text>
        </Flex>

        {status?.configured ? (
          <Alert
            type="info"
            showIcon
            icon={<CloudServerOutlined />}
            message={<Text ellipsis style={{ maxWidth: 260 }}>{status.base_url}</Text>}
            action={
              <Button size="small" type="text" icon={<SettingOutlined />} onClick={() => setShowServer(true)} />
            }
            style={{ marginBottom: 16 }}
          />
        ) : (
          <Alert
            type="warning"
            showIcon
            message="Chưa cấu hình địa chỉ máy chủ."
            action={
              <Button size="small" onClick={() => setShowServer(true)}>
                Cài đặt
              </Button>
            }
            style={{ marginBottom: 16 }}
          />
        )}

        {err && <Alert type="error" showIcon message={err} style={{ marginBottom: 16 }} />}

        <Form
          layout="vertical"
          onFinish={login}
          initialValues={{ username: status?.username ?? '' }}
          requiredMark={false}
        >
          <Form.Item
            name="username"
            rules={[{ required: true, message: 'Nhập tài khoản (email)' }]}
          >
            <Input prefix={<UserOutlined />} placeholder="Tài khoản (email)" size="large" />
          </Form.Item>
          <Form.Item name="password" rules={[{ required: true, message: 'Nhập mật khẩu' }]}>
            <Input.Password prefix={<LockOutlined />} placeholder="Mật khẩu" size="large" />
          </Form.Item>
          <Button
            type="primary"
            htmlType="submit"
            size="large"
            block
            loading={busy}
            disabled={!status?.configured}
            icon={<ApiOutlined />}
          >
            Đăng nhập
          </Button>
        </Form>
      </Card>

      <ServerModal
        open={showServer}
        initial={status?.base_url ?? ''}
        onClose={() => setShowServer(false)}
        onSaved={onStatus}
      />
    </Flex>
  )
}

function ServerModal({
  open,
  initial,
  onClose,
  onSaved,
}: {
  open: boolean
  initial: string
  onClose: () => void
  onSaved: (s: ConnStatus) => void
}) {
  const [form] = Form.useForm<{ base_url: string; namespace: string }>()
  const [busy, setBusy] = useState(false)

  const save = async (values: { base_url: string; namespace: string }) => {
    setBusy(true)
    try {
      const st = await api.saveSettings({
        base_url: values.base_url,
        namespace: values.namespace ?? '',
      })
      onSaved(st)
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={open}
      title={
        <Space>
          <CloudServerOutlined /> Máy chủ Dipper Hub
        </Space>
      }
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Form
        form={form}
        layout="vertical"
        onFinish={save}
        initialValues={{ base_url: initial, namespace: '' }}
        requiredMark={false}
      >
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
        <Button type="primary" htmlType="submit" loading={busy} block>
          Lưu
        </Button>
      </Form>
    </Modal>
  )
}
