import { useCallback, useEffect, useMemo, useState } from 'react'
import { ConfigProvider, Layout, Menu, Button, Space, Tooltip, theme as antdTheme } from 'antd'
import type { MenuProps } from 'antd'
import viVN from 'antd/locale/vi_VN'
import enUS from 'antd/locale/en_US'
import {
  RobotOutlined, ApiOutlined, MessageOutlined, InboxOutlined, WarningOutlined,
  BarChartOutlined, ReadOutlined, SettingOutlined, BulbOutlined, BulbFilled, GlobalOutlined,
} from '@ant-design/icons'
import { api } from './api'
import type { Bot } from './api'
import { makeT } from './i18n'
import type { Lang } from './i18n'
import Logo from './Logo'
import BotsPage from './pages/Bots'
import ChannelsPage from './pages/Channels'
import ChatPage from './pages/Chat'
import InboxPage from './pages/Inbox'
import IssuesPage from './pages/Issues'
import AnalyticsPage from './pages/Analytics'
import KnowledgePage from './pages/Knowledge'
import SettingsPage from './pages/Settings'

const { Header, Sider, Content } = Layout
type Tab = 'analytics' | 'chat' | 'inbox' | 'issues' | 'bots' | 'channels' | 'knowledge' | 'settings'
type Mode = 'light' | 'dark'

export default function App() {
  const [lang, setLang] = useState<Lang>('vi')
  const [mode, setMode] = useState<Mode>(() =>
    window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  )
  const [tab, setTab] = useState<Tab>('analytics')
  const [bots, setBots] = useState<Bot[]>([])
  const t = useMemo(() => makeT(lang), [lang])

  const refreshBots = useCallback(async () => {
    try {
      setBots(await api.listBots())
    } catch {
      /* daemon offline — pages surface their own errors */
    }
  }, [])

  useEffect(() => {
    refreshBots()
    api.getSettings().then((s) => setLang((s.language as Lang) || 'vi')).catch(() => {})
  }, [refreshBots])

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', mode)
  }, [mode])

  // Grouped nav, mirroring the reference (Overview / Support / Build / System).
  const menuItems = useMemo(
    () => [
      { type: 'group' as const, label: t('groupOverview'), children: [{ key: 'analytics', icon: <BarChartOutlined />, label: t('tabAnalytics') }] },
      {
        type: 'group' as const,
        label: t('groupSupport'),
        children: [
          { key: 'chat', icon: <MessageOutlined />, label: t('tabChat') },
          { key: 'inbox', icon: <InboxOutlined />, label: t('tabInbox') },
          { key: 'issues', icon: <WarningOutlined />, label: t('tabIssues') },
        ],
      },
      {
        type: 'group' as const,
        label: t('groupBuild'),
        children: [
          { key: 'bots', icon: <RobotOutlined />, label: t('tabBots') },
          { key: 'channels', icon: <ApiOutlined />, label: t('tabChannels') },
          { key: 'knowledge', icon: <ReadOutlined />, label: t('tabKnowledge') },
        ],
      },
      { type: 'group' as const, label: t('groupSystem'), children: [{ key: 'settings', icon: <SettingOutlined />, label: t('tabSettings') }] },
    ],
    [t],
  )

  return (
    <ConfigProvider
      locale={lang === 'vi' ? viVN : enUS}
      theme={{ algorithm: mode === 'dark' ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm, token: { colorPrimary: '#1890ff', borderRadius: 6 } }}
    >
      <Shell
        mode={mode}
        setMode={setMode}
        lang={lang}
        setLang={setLang}
        t={t}
        tab={tab}
        setTab={setTab}
        bots={bots}
        refreshBots={refreshBots}
        menuItems={menuItems}
      />
    </ConfigProvider>
  )
}

/** Inner shell so it can read theme tokens (colorBgContainer, primary, …). */
function Shell(props: {
  mode: Mode
  setMode: (m: Mode) => void
  lang: Lang
  setLang: (l: Lang) => void
  t: ReturnType<typeof makeT>
  tab: Tab
  setTab: (tb: Tab) => void
  bots: Bot[]
  refreshBots: () => void
  menuItems: MenuProps['items']
}) {
  const { mode, setMode, lang, setLang, t, tab, setTab, bots, refreshBots, menuItems } = props
  const { token } = antdTheme.useToken()
  const [online, setOnline] = useState(false)

  useEffect(() => {
    let alive = true
    const ping = () => api.status().then(() => alive && setOnline(true)).catch(() => alive && setOnline(false))
    ping()
    const id = setInterval(ping, 15000)
    return () => { alive = false; clearInterval(id) }
  }, [])

  const isDark = mode === 'dark'
  return (
    <Layout style={{ height: '100vh' }}>
      <Sider width={210} theme={mode} breakpoint="lg" collapsedWidth={0}>
        <div className="logo">
          <Logo size={28} textColor={isDark ? '#ffffff' : '#0b1f33'} />
        </div>
        <Menu
          theme={mode}
          mode="inline"
          selectedKeys={[tab]}
          items={menuItems}
          onClick={(e) => setTab(e.key as Tab)}
          style={{ borderInlineEnd: 0 }}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            padding: '0 24px',
            background: token.colorBgContainer,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <h2 style={{ margin: 0, color: token.colorPrimary }}>{t('appName')}</h2>
          <Space size="middle">
            <span style={{ fontSize: 12, color: online ? '#52c41a' : '#ff4d4f', display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 8, height: 8, borderRadius: '50%', background: online ? '#52c41a' : '#ff4d4f' }} />
              {online ? t('connected') : t('disconnected')}
            </span>
            <Button type="text" icon={<GlobalOutlined />} onClick={() => setLang(lang === 'vi' ? 'en' : 'vi')}>
              {lang === 'vi' ? 'VI' : 'EN'}
            </Button>
            <Tooltip title={isDark ? 'Light' : 'Dark'}>
              <Button type="text" aria-label="Toggle theme" icon={isDark ? <BulbFilled /> : <BulbOutlined />} onClick={() => setMode(isDark ? 'light' : 'dark')} />
            </Tooltip>
          </Space>
        </Header>
        <Content style={{ margin: '24px 16px', padding: 24, background: token.colorBgContainer, borderRadius: token.borderRadius, overflow: 'auto', flex: 1, minHeight: 0 }}>
          {tab === 'analytics' && <AnalyticsPage t={t} />}
          {tab === 'chat' && <ChatPage t={t} bots={bots} />}
          {tab === 'inbox' && <InboxPage t={t} bots={bots} />}
          {tab === 'issues' && <IssuesPage t={t} bots={bots} />}
          {tab === 'bots' && <BotsPage t={t} bots={bots} refresh={refreshBots} />}
          {tab === 'channels' && <ChannelsPage t={t} bots={bots} />}
          {tab === 'knowledge' && <KnowledgePage t={t} bots={bots} />}
          {tab === 'settings' && <SettingsPage t={t} lang={lang} setLang={setLang} />}
        </Content>
      </Layout>
    </Layout>
  )
}
