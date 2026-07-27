import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Avatar, Button, ConfigProvider, Layout, Menu, Tag, Tooltip, theme } from 'antd'
import {
  BarChartOutlined,
  EditOutlined,
  MoonOutlined,
  SearchOutlined,
  SettingOutlined,
  SunOutlined,
} from '@ant-design/icons'
import { api, type Identity, type Status } from './api'
import { SearchPage } from './pages/SearchPage'
import { DashboardPage } from './pages/DashboardPage'
import { DraftsPage } from './pages/DraftsPage'
import { SettingsPage } from './pages/SettingsPage'

const ACCENT = '#ff0033'
const FONT = "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"

export type View = 'search' | 'dashboard' | 'drafts' | 'settings'
type Theme = 'light' | 'dark'

function readTheme(): Theme {
  try {
    const s = localStorage.getItem('yt-theme')
    if (s === 'light' || s === 'dark') return s
  } catch {
    /* ignore */
  }
  return window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export default function App() {
  const [themeMode, setThemeMode] = useState<Theme>(readTheme)

  useEffect(() => {
    try {
      localStorage.setItem('yt-theme', themeMode)
    } catch {
      /* ignore */
    }
    document.body.style.background = themeMode === 'dark' ? '#141414' : '#f5f5f5'
    document.documentElement.dataset.theme = themeMode
  }, [themeMode])

  return (
    <ConfigProvider
      theme={{
        algorithm: themeMode === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: ACCENT,
          colorInfo: ACCENT,
          colorSuccess: '#34c759',
          colorWarning: '#ff9500',
          colorError: '#ff3b30',
          borderRadius: 8,
          fontFamily: FONT,
        },
      }}
    >
      <AntApp>
        <Shell themeMode={themeMode} setThemeMode={setThemeMode} />
      </AntApp>
    </ConfigProvider>
  )
}

function Shell({ themeMode, setThemeMode }: { themeMode: Theme; setThemeMode: (t: Theme) => void }) {
  const [view, setView] = useState<View>('search')
  const [status, setStatus] = useState<Status | null>(null)
  const [model, setModel] = useState<string | null>(null)
  const [identity, setIdentity] = useState<Identity | null>(null)

  const refresh = useCallback(async () => {
    try {
      const [s, l, o] = await Promise.all([api.status(), api.llmInfo(), api.oauthStatus()])
      setStatus(s)
      setModel(l.model ?? null)
      setIdentity(o.identity)
    } catch {
      /* ignore transient errors */
    }
  }, [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 5000)
    return () => clearInterval(t)
  }, [refresh])

  const connected = status?.status.extensionConnected ?? false
  const loggedIn = Boolean(status?.status.auth?.hasSapisid || status?.status.auth?.loggedIn)

  const menuItems = [
    { key: 'search', icon: <SearchOutlined />, label: 'Tìm kiếm' },
    { key: 'dashboard', icon: <BarChartOutlined />, label: 'Bình luận & thống kê' },
    { key: 'drafts', icon: <EditOutlined />, label: 'Bản nháp' },
    { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
  ]

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Layout.Sider theme={themeMode} breakpoint="lg" collapsedWidth="0" width={220}>
        <div style={{ fontWeight: 700, fontSize: 16, padding: '18px 20px 12px', whiteSpace: 'nowrap' }}>
          ▶️ SenClaw YouTube
        </div>
        <Menu
          theme={themeMode}
          mode="inline"
          selectedKeys={[view]}
          items={menuItems}
          onClick={(e) => setView(e.key as View)}
        />
      </Layout.Sider>

      <Layout>
        <Layout.Header
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '0 24px',
            background: 'transparent',
            borderBottom: '1px solid rgba(128,128,128,0.2)',
          }}
        >
          <Tag color={connected ? 'success' : 'default'}>
            {connected ? 'Extension đã kết nối' : 'Extension chưa kết nối'}
          </Tag>
          <Tag color={loggedIn ? 'success' : 'warning'}>
            {loggedIn ? 'Đã đăng nhập YouTube' : 'Chưa thấy phiên đăng nhập'}
          </Tag>
          <Tag>{model ? `LLM: ${model}` : 'LLM: —'}</Tag>

          <span style={{ flex: 1 }} />

          {identity ? (
            <Tooltip title={`Đã đăng nhập Google: ${identity.title}`}>
              <Tag
                color="success"
                style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 6 }}
                onClick={() => setView('settings')}
              >
                {identity.thumbnail && <Avatar size={18} src={identity.thumbnail} />}
                {identity.title || 'Google'}
              </Tag>
            </Tooltip>
          ) : (
            <Button size="small" onClick={() => setView('settings')}>
              Đăng nhập Google
            </Button>
          )}

          <Tooltip title={themeMode === 'dark' ? 'Chế độ sáng' : 'Chế độ tối'}>
            <Button
              type="text"
              icon={themeMode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
              onClick={() => setThemeMode(themeMode === 'dark' ? 'light' : 'dark')}
            />
          </Tooltip>
        </Layout.Header>

        <Layout.Content style={{ padding: 24, maxWidth: 1100, width: '100%', margin: '0 auto' }}>
          {view === 'search' && <SearchPage connected={connected} onView={setView} />}
          {view === 'dashboard' && <DashboardPage connected={connected} />}
          {view === 'drafts' && <DraftsPage />}
          {view === 'settings' && <SettingsPage status={status} model={model} onChanged={refresh} />}
        </Layout.Content>
      </Layout>
    </Layout>
  )
}
