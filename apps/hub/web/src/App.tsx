import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Avatar,
  Badge,
  Button,
  ConfigProvider,
  Dropdown,
  Flex,
  Layout,
  Menu,
  Spin,
  Typography,
  theme as antTheme,
} from 'antd'
import {
  AlertOutlined,
  AppstoreOutlined,
  CodeOutlined,
  LogoutOutlined,
  MoonOutlined,
  SettingOutlined,
  SunOutlined,
  UserOutlined,
} from '@ant-design/icons'
import Logo from './Logo'
import viVN from 'antd/locale/vi_VN'
import { api, type ConnStatus } from './api'
import { POLL_MS } from './ui'
import Login from './views/Login'
import Dashboard from './views/Dashboard'
import DeviceDetail from './views/DeviceDetail'
import Panels from './views/Panels'
import Alerts from './views/Alerts'
import Settings from './views/Settings'

const { Text, Title } = Typography

type View =
  | { kind: 'dashboard' }
  | { kind: 'device'; id: string }
  | { kind: 'panels' }
  | { kind: 'alerts' }
  | { kind: 'settings' }

export default function App() {
  const [dark, setDark] = useState(() => localStorage.getItem('hub-theme') !== 'light')
  useEffect(() => {
    localStorage.setItem('hub-theme', dark ? 'dark' : 'light')
  }, [dark])

  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: dark ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: { colorPrimary: '#4da3ff', borderRadius: 8 },
      }}
    >
      <AntApp style={{ height: '100%' }}>
        <Shell dark={dark} onToggleTheme={() => setDark((d) => !d)} />
      </AntApp>
    </ConfigProvider>
  )
}

function Shell({ dark, onToggleTheme }: { dark: boolean; onToggleTheme: () => void }) {
  const { token } = antTheme.useToken()
  const [status, setStatus] = useState<ConnStatus | null>(null)
  const [booted, setBooted] = useState(false)
  const [view, setView] = useState<View>({ kind: 'dashboard' })
  const [collapsed, setCollapsed] = useState(false)

  const refreshStatus = useCallback(() => {
    api
      .status()
      .then(setStatus)
      .catch(() => setStatus(null))
      .finally(() => setBooted(true))
  }, [])

  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, POLL_MS * 2)
    return () => clearInterval(t)
  }, [refreshStatus])

  if (!booted)
    return (
      <Flex align="center" justify="center" style={{ height: '100vh' }}>
        <Spin size="large" />
      </Flex>
    )

  // Dedicated full-screen login until we have a live session.
  if (!status?.connected) {
    return <Login status={status} onStatus={setStatus} />
  }

  const menuKey = view.kind === 'device' ? 'dashboard' : view.kind

  const logout = async () => {
    const st = await api.logout()
    setStatus(st)
    setView({ kind: 'dashboard' })
  }

  return (
    <Layout style={{ height: '100vh' }}>
      <Layout.Sider
        collapsible
        collapsed={collapsed}
        onCollapse={setCollapsed}
        width={210}
        style={{ borderRight: `1px solid ${token.colorBorderSecondary}` }}
      >
        <Flex
          align="center"
          justify={collapsed ? 'center' : 'flex-start'}
          gap={10}
          style={{ padding: '16px 16px 12px' }}
        >
          <Logo size={26} />
          {!collapsed && (
            <Title level={5} style={{ margin: 0, whiteSpace: 'nowrap' }}>
              Device Hub
            </Title>
          )}
        </Flex>
        <Menu
          mode="inline"
          selectedKeys={[menuKey]}
          onClick={({ key }) => setView({ kind: key } as View)}
          items={[
            { key: 'dashboard', icon: <AppstoreOutlined />, label: 'Thiết bị' },
            { key: 'panels', icon: <CodeOutlined />, label: 'Panel HMI' },
            { key: 'alerts', icon: <AlertOutlined />, label: 'Cảnh báo' },
            { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
          ]}
          style={{ borderInlineEnd: 'none' }}
        />
      </Layout.Sider>
      <Layout>
        <Layout.Header
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 14,
            padding: '0 20px',
            background: token.colorBgContainer,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          <Badge status="success" text={<Text type="secondary">{status.base_url}</Text>} />
          <div style={{ flex: 1 }} />
          <Button
            type="text"
            icon={dark ? <SunOutlined /> : <MoonOutlined />}
            onClick={onToggleTheme}
            title={dark ? 'Chế độ sáng' : 'Chế độ tối'}
          />
          <Dropdown
            menu={{
              items: [
                {
                  key: 'logout',
                  icon: <LogoutOutlined />,
                  label: 'Đăng xuất',
                  onClick: logout,
                },
              ],
            }}
          >
            <Flex align="center" gap={8} style={{ cursor: 'pointer' }}>
              <Avatar size="small" icon={<UserOutlined />} />
              <Text>{status.username}</Text>
            </Flex>
          </Dropdown>
        </Layout.Header>
        <Layout.Content style={{ padding: 20, overflow: 'auto' }}>
          {view.kind === 'dashboard' && (
            <Dashboard onOpen={(id) => setView({ kind: 'device', id })} />
          )}
          {view.kind === 'device' && (
            <DeviceDetail id={view.id} onBack={() => setView({ kind: 'dashboard' })} />
          )}
          {view.kind === 'panels' && <Panels />}
          {view.kind === 'alerts' && <Alerts />}
          {view.kind === 'settings' && <Settings status={status} onStatus={setStatus} />}
        </Layout.Content>
      </Layout>
    </Layout>
  )
}
