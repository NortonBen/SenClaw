import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  ConfigProvider,
  Layout,
  Menu,
  Space,
  Switch,
  Tag,
  Tooltip,
  theme as antTheme,
} from 'antd'
import type { MenuProps } from 'antd'
import {
  ApiOutlined,
  DashboardOutlined,
  FileTextOutlined,
  GlobalOutlined,
  MessageOutlined,
  MoonOutlined,
  SettingOutlined,
  SunOutlined,
  TeamOutlined,
} from '@ant-design/icons'
import { getStatus, type Status } from './api'
import { useThemeMode } from './theme'
import Dashboard from './pages/Dashboard'
import Drafts from './pages/Drafts'
import Inbox from './pages/Inbox'
import Accounts from './pages/Accounts'
import Logs from './pages/Logs'
import Platforms from './pages/Platforms'
import Settings from './pages/Settings'

const { Sider, Header, Content } = Layout

type Key = 'dashboard' | 'drafts' | 'inbox' | 'accounts' | 'logs' | 'platforms' | 'settings'
const TITLES: Record<Key, [string, string]> = {
  dashboard: ['Bảng điều khiển', 'Tình trạng hệ thống và hoạt động gần đây'],
  drafts: ['Nháp chờ duyệt', 'Bài đăng / trả lời tạo ở chế độ draft phải duyệt trước khi gửi'],
  inbox: ['Hộp thư', 'Tin nhắn đã thu và các câu trả lời đã gửi'],
  accounts: ['Tài khoản', 'Tài khoản đã kết nối và cấu hình API chính thức'],
  logs: ['Lịch sử & audit', 'Hành động API, lượt đăng, và phiên đăng nhập'],
  platforms: ['Nền tảng', 'Mỗi nền tảng làm được gì, và bằng đường nào'],
  settings: ['Cài đặt', 'Chế độ tự chủ, extension, và ranh giới an toàn'],
}

function currentKey(): Key {
  const k = (location.hash.replace('#/', '') || 'dashboard') as Key
  return TITLES[k] ? k : 'dashboard'
}

export default function App() {
  const { mode, toggle } = useThemeMode()
  const [key, setKey] = useState<Key>(currentKey)
  const [status, setStatus] = useState<Status | null>(null)

  const refresh = useCallback(async () => setStatus(await getStatus()), [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 6000)
    const onHash = () => setKey(currentKey())
    window.addEventListener('hashchange', onHash)
    return () => {
      clearInterval(t)
      window.removeEventListener('hashchange', onHash)
    }
  }, [refresh])

  const items: MenuProps['items'] = [
    {
      type: 'group',
      label: 'Tổng quan',
      children: [{ key: 'dashboard', icon: <DashboardOutlined />, label: 'Bảng điều khiển' }],
    },
    {
      type: 'group',
      label: 'Vận hành',
      children: [
        {
          key: 'drafts',
          icon: <FileTextOutlined />,
          label: (
            <Space size={6}>
              Nháp chờ duyệt
              {!!status?.drafts_pending && <Tag color="blue">{status.drafts_pending}</Tag>}
            </Space>
          ),
        },
        { key: 'inbox', icon: <MessageOutlined />, label: 'Hộp thư' },
        {
          key: 'accounts',
          icon: <TeamOutlined />,
          label: (
            <Space size={6}>
              Tài khoản
              {!!status?.accounts && <Tag>{status.accounts}</Tag>}
            </Space>
          ),
        },
      ],
    },
    {
      type: 'group',
      label: 'Nhật ký',
      children: [{ key: 'logs', icon: <ApiOutlined />, label: 'Lịch sử & audit' }],
    },
    {
      type: 'group',
      label: 'Hệ thống',
      children: [
        { key: 'platforms', icon: <GlobalOutlined />, label: 'Nền tảng' },
        { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
      ],
    },
  ]

  const [title, subtitle] = TITLES[key]
  const modeColor = status?.autonomy === 'live' ? 'orange' : status?.autonomy === 'observe' ? 'purple' : 'green'

  return (
    <ConfigProvider
      theme={{
        algorithm: mode === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: { colorPrimary: '#3b82f6', borderRadius: 8 },
      }}
    >
      <AntApp>
        <Layout style={{ minHeight: '100vh' }}>
          <Sider breakpoint="lg" collapsedWidth="0" width={218} theme={mode}>
            <div style={{ padding: '16px 18px', fontWeight: 700, fontSize: 16 }}>📣 Social</div>
            <Menu
              theme={mode}
              mode="inline"
              selectedKeys={[key]}
              items={items}
              onClick={(e) => {
                location.hash = `#/${e.key}`
                setKey(e.key as Key)
              }}
            />
          </Sider>
          <Layout>
            <Header
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 16,
                padding: '0 20px',
                background: 'transparent',
                height: 56,
                lineHeight: '56px',
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 17, fontWeight: 600, lineHeight: 1.3 }}>{title}</div>
                <div style={{ fontSize: 12, opacity: 0.65, lineHeight: 1.3 }}>{subtitle}</div>
              </div>
              <Space wrap>
                {status && <Tag color={modeColor}>chế độ: {status.autonomy}</Tag>}
                {status && (
                  <Tag color={status.extension_connected ? 'green' : 'red'}>
                    extension: {status.extension_connected ? 'đã kết nối' : 'chưa kết nối'}
                  </Tag>
                )}
                {!!status?.extension_hosts_ready?.length && (
                  <Tag color="green">phiên: {status.extension_hosts_ready.join(', ')}</Tag>
                )}
                <Tooltip title={mode === 'dark' ? 'Chuyển sang giao diện sáng' : 'Chuyển sang giao diện tối'}>
                  <Switch
                    checked={mode === 'dark'}
                    onChange={toggle}
                    checkedChildren={<MoonOutlined />}
                    unCheckedChildren={<SunOutlined />}
                  />
                </Tooltip>
              </Space>
            </Header>
            <Content style={{ padding: '4px 20px 40px' }}>
              {key === 'dashboard' && <Dashboard status={status} />}
              {key === 'drafts' && <Drafts status={status} onChanged={refresh} />}
              {key === 'inbox' && <Inbox />}
              {key === 'accounts' && <Accounts status={status} onChanged={refresh} />}
              {key === 'logs' && <Logs status={status} />}
              {key === 'platforms' && <Platforms status={status} />}
              {key === 'settings' && <Settings status={status} onChanged={refresh} />}
            </Content>
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  )
}
