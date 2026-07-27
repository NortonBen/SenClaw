import { useCallback, useEffect, useState } from 'react'
import { Alert, App, Avatar, Button, Card, Form, Input, Select, Space, Tag, Typography } from 'antd'
import { GoogleOutlined, LogoutOutlined } from '@ant-design/icons'
import { api, type ModelInfo, type OAuthStatus, type Status } from '../api'

export function SettingsPage({
  status,
  model,
  onChanged,
}: {
  status: Status | null
  model: string | null
  onChanged: () => void
}) {
  const { message } = App.useApp()
  const [models, setModels] = useState<ModelInfo[]>([])
  const [activeId, setActiveId] = useState<string>('')
  const [oauth, setOauth] = useState<OAuthStatus | null>(null)
  const [savingOauth, setSavingOauth] = useState(false)

  const loadOauth = useCallback(() => {
    api.oauthStatus().then(setOauth).catch(() => {})
  }, [])

  useEffect(() => {
    api.models().then((m) => { setModels(m.configs); setActiveId(m.activeId) }).catch(() => {})
    loadOauth()
    const t = setInterval(loadOauth, 4000)
    return () => clearInterval(t)
  }, [loadOauth])

  const connected = status?.status.extensionConnected ?? false
  const loggedIn = Boolean(status?.status.auth?.hasSapisid || status?.status.auth?.loggedIn)

  const pickModel = async (id: string) => {
    try {
      await api.setModel(id)
      setActiveId(id)
      message.success('Đã đổi model')
      onChanged()
    } catch (e) {
      message.error(String(e))
    }
  }

  const saveOauth = async (v: { client_id: string; client_secret: string }) => {
    setSavingOauth(true)
    try {
      const r = await api.oauthConfig(v.client_id, v.client_secret)
      message.success('Đã lưu client. Mở trang đăng nhập Google…')
      window.open(r.authUrl, '_blank')
      loadOauth()
    } catch (e) {
      message.error(String(e))
    } finally {
      setSavingOauth(false)
    }
  }

  const signIn = () => {
    window.open('/api/oauth/start', '_blank')
    message.info('Hoàn tất đăng nhập ở tab mới rồi quay lại.')
  }

  const signOut = async () => {
    try {
      await api.oauthLogout()
      message.success('Đã đăng xuất Google')
      loadOauth()
      onChanged()
    } catch (e) {
      message.error(String(e))
    }
  }

  const id = oauth?.identity

  return (
    <Space direction="vertical" size="large" style={{ width: '100%', maxWidth: 720 }}>
      <Typography.Title level={4} style={{ margin: 0 }}>
        Cài đặt
      </Typography.Title>

      <Card size="small" title="Kết nối">
        <Space wrap>
          <Tag color={connected ? 'success' : 'default'}>
            {connected ? 'Extension đã kết nối' : 'Extension chưa kết nối'}
          </Tag>
          <Tag color={loggedIn ? 'success' : 'warning'}>
            {loggedIn ? 'Đã đăng nhập YouTube' : 'Chưa thấy phiên đăng nhập'}
          </Tag>
        </Space>
        <Alert
          style={{ marginTop: 12 }}
          type="info"
          showIcon
          message="Cài extension"
          description={
            <>
              Cài <code>apps/youtube/extension</code> vào Chrome, mở YouTube đã đăng nhập, và đặt{' '}
              <b>WS port 9223</b> + HTTP port của app trong popup extension. Đọc/đăng chỉ hoạt động khi extension
              kết nối.
            </>
          }
        />
      </Card>

      <Card size="small" title="Mô hình LLM">
        <Select
          style={{ minWidth: 320 }}
          value={activeId || undefined}
          placeholder={model || 'Chọn model'}
          onChange={pickModel}
          options={models.map((m) => ({ value: m.id, label: `${m.modelName} (${m.provider})` }))}
        />
      </Card>

      <Card size="small" title="Đăng nhập Google (YouTube Data API — cho kiểm duyệt)">
        {/* Signed in */}
        {oauth?.authorized && id ? (
          <Space direction="vertical" style={{ width: '100%' }}>
            <Space>
              <Avatar src={id.thumbnail || undefined} icon={<GoogleOutlined />} />
              <span>
                Đã đăng nhập: <b>{id.title || id.channelId}</b>
              </span>
              <Tag color="success">Kiểm duyệt sẵn sàng</Tag>
            </Space>
            <Button danger icon={<LogoutOutlined />} onClick={signOut}>
              Đăng xuất
            </Button>
          </Space>
        ) : oauth?.configured ? (
          /* Configured but not authorized */
          <Space direction="vertical">
            <Typography.Text type="secondary">Đã cấu hình client — bấm để đăng nhập.</Typography.Text>
            <Button type="primary" icon={<GoogleOutlined />} onClick={signIn}>
              Đăng nhập với Google
            </Button>
          </Space>
        ) : (
          /* Not configured yet */
          <>
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 12 }}
              message="Cần một OAuth client kiểu Desktop"
              description={
                <>
                  Tạo trên Google Cloud Console (scope <code>youtube.force-ssl</code>), dán id/secret rồi bấm để
                  đăng nhập. Redirect URI:{' '}
                  <code>{oauth?.redirectUri || 'http://127.0.0.1:<port>/api/oauth/callback'}</code>
                </>
              }
            />
            <Form layout="vertical" onFinish={saveOauth}>
              <Form.Item name="client_id" label="Client ID" rules={[{ required: true }]}>
                <Input placeholder="xxxxx.apps.googleusercontent.com" />
              </Form.Item>
              <Form.Item name="client_secret" label="Client Secret" rules={[{ required: true }]}>
                <Input.Password placeholder="GOCSPX-…" />
              </Form.Item>
              <Button type="primary" htmlType="submit" icon={<GoogleOutlined />} loading={savingOauth}>
                Lưu & đăng nhập
              </Button>
            </Form>
          </>
        )}
      </Card>
    </Space>
  )
}
