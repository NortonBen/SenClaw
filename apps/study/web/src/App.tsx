import { useEffect, useState } from 'react'
import { Badge, Layout, Menu, Segmented, Tooltip, Typography, theme as antdTheme } from 'antd'
import {
  BookOutlined,
  BulbOutlined,
  CalendarOutlined,
  DesktopOutlined,
  FileTextOutlined,
  MoonOutlined,
  QuestionCircleOutlined,
  SettingOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import { useTheme, type ThemeMode } from './theme'
import DocsView from './docs'
import PlansView from './plans'
import TodayView from './today'
import CardsView from './cards'
import AskView from './ask'
import SettingsView from './settings'
import { get } from './api'

type Tab = 'today' | 'docs' | 'plans' | 'cards' | 'ask' | 'settings'

/** Session id from the deep link a calendar event carries
 *  (`/space/app/study?session=…`, forwarded into the iframe by the host). */
function deepLinkSession(): string | null {
  const q = new URLSearchParams(window.location.search)
  const id = q.get('session')
  return id && id.trim() ? id.trim() : null
}

export default function App() {
  const [tab, setTab] = useState<Tab>('today')
  const [openSession, setOpenSession] = useState<string | null>(deepLinkSession)
  const [due, setDue] = useState(0)
  const { resolved } = useTheme()
  const { token } = antdTheme.useToken()

  useEffect(() => {
    const load = () =>
      get<{ cardsDue: number }>('/status')
        .then((s) => setDue(s.cardsDue ?? 0))
        .catch(() => {})
    load()
    const t = setInterval(load, 60_000)
    return () => clearInterval(t)
  }, [tab])

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Layout.Header
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 16,
          paddingInline: 16,
          // In light mode the header follows the surface instead of staying a
          // dark navy bar the rest of the page has nothing to do with.
          background: resolved === 'dark' ? token.colorBgContainer : token.colorBgElevated,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <Typography.Text
          strong
          style={{ color: token.colorText, fontSize: 16, whiteSpace: 'nowrap' }}
        >
          📚 Study
        </Typography.Text>
        <Menu
          theme={resolved}
          mode="horizontal"
          selectedKeys={[tab]}
          onClick={(e) => {
            setTab(e.key as Tab)
            if (e.key !== 'today') setOpenSession(null)
          }}
          style={{ flex: 1, minWidth: 0 }}
          items={[
            { key: 'today', icon: <CalendarOutlined />, label: 'Hôm nay' },
            { key: 'docs', icon: <FileTextOutlined />, label: 'Tài liệu' },
            { key: 'plans', icon: <BookOutlined />, label: 'Lộ trình' },
            {
              key: 'cards',
              icon: <ThunderboltOutlined />,
              label: <Badge count={due} size="small" offset={[8, -2]}>Thẻ ôn</Badge>,
            },
            { key: 'ask', icon: <QuestionCircleOutlined />, label: 'Hỏi tài liệu' },
            { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
          ]}
        />
        <ThemeSwitch />
      </Layout.Header>
      <Layout.Content style={{ padding: 16 }}>
        {tab === 'today' && (
          <TodayView sessionId={openSession} onOpen={setOpenSession} />
        )}
        {tab === 'docs' && <DocsView />}
        {tab === 'plans' && <PlansView onOpenSession={(id) => { setOpenSession(id); setTab('today') }} />}
        {tab === 'cards' && <CardsView />}
        {tab === 'ask' && <AskView />}
        {tab === 'settings' && <SettingsView />}
      </Layout.Content>
    </Layout>
  )
}

/** Auto / light / dark. `Auto` follows the SenClaw shell when embedded, the OS
 *  otherwise; the other two are the reader's own call and outrank both. */
function ThemeSwitch() {
  const { mode, setMode, embedded } = useTheme()
  return (
    <Tooltip
      title={
        embedded
          ? 'Tự động = theo giao diện SenClaw. Chọn Sáng/Tối để đọc theo ý bạn.'
          : 'Tự động = theo hệ điều hành.'
      }
    >
      <Segmented
        size="small"
        value={mode}
        onChange={(v) => setMode(v as ThemeMode)}
        options={[
          { value: 'system', icon: <DesktopOutlined />, title: 'Tự động' },
          { value: 'light', icon: <BulbOutlined />, title: 'Sáng' },
          { value: 'dark', icon: <MoonOutlined />, title: 'Tối' },
        ]}
      />
    </Tooltip>
  )
}
