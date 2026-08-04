import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Layout, Menu, Tag, theme, Typography } from 'antd'
import {
  ExperimentOutlined,
  HistoryOutlined,
  LoadingOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SettingOutlined,
} from '@ant-design/icons'
import { Button } from 'antd'
import { api, type SourceInfo } from './api'
import SearchPage from './SearchPage'
import HistoryPage from './HistoryPage'
import SettingsPage from './SettingsPage'

const { Header, Sider, Content } = Layout
const { Title } = Typography

type Page = 'search' | 'history' | 'settings'

export default function App() {
  const { token } = theme.useToken()
  const { message } = AntApp.useApp()
  const [collapsed, setCollapsed] = useState(false)

  const [page, setPage] = useState<Page>('search')
  const [sources, setSources] = useState<SourceInfo[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [searching, setSearching] = useState(false)

  const nav = [
    {
      key: 'search',
      icon: searching ? <LoadingOutlined spin /> : <ExperimentOutlined />,
      label: 'Nghiên cứu',
    },
    { key: 'history', icon: <HistoryOutlined />, label: 'Lịch sử' },
    { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
  ]

  const loadSources = useCallback(
    (resetSelection: boolean) =>
      api
        .sources()
        .then((next) => {
          setSources((previous) => {
            if (resetSelection) {
              setSelected(new Set(next.sources.filter((s) => s.enabled).map((s) => s.id)))
            } else {
              const before = new Set(previous.map((s) => s.id))
              const now = new Set(next.sources.map((s) => s.id))
              setSelected((picked) => {
                const merged = new Set([...picked].filter((id) => now.has(id)))
                for (const s of next.sources) {
                  if (!before.has(s.id) && s.enabled) merged.add(s.id)
                }
                return merged
              })
            }
            return next.sources
          })
        })
        .catch((e: Error) => message.error(e.message)),
    [message],
  )

  useEffect(() => {
    loadSources(true)
  }, [loadSources])

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  return (
    <Layout style={{ minHeight: '100vh', background: token.colorBgLayout }}>
      <Sider
        theme="light"
        collapsedWidth={56}
        collapsed={collapsed}
        trigger={null}
        width={208}
        style={{
          background: token.colorBgContainer,
          borderRight: `1px solid ${token.colorBorderSecondary}`,
          position: 'sticky',
          top: 0,
          height: '100vh',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            height: 56,
            paddingInline: collapsed ? 0 : 18,
            justifyContent: collapsed ? 'center' : 'flex-start',
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            overflow: 'hidden',
          }}
        >
          <ExperimentOutlined style={{ color: token.colorPrimary, fontSize: 20, flexShrink: 0 }} />
          {!collapsed && (
            <Title level={4} style={{ margin: 0, whiteSpace: 'nowrap' }}>
              Zeach
            </Title>
          )}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[page]}
          onClick={({ key }) => setPage(key as Page)}
          items={nav}
          style={{ borderInlineEnd: 0, background: 'transparent', marginTop: 6 }}
        />
      </Sider>

      <Layout style={{ background: token.colorBgLayout }}>
        <Header
          style={{
            display: 'flex',
            alignItems: 'center',
            background: token.colorBgContainer,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            paddingInline: 24,
            height: 56,
            position: 'sticky',
            top: 0,
            zIndex: 10,
          }}
        >
          <Button
            type="text"
            icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            onClick={() => setCollapsed((v) => !v)}
            style={{ marginRight: 12 }}
          />
          <Title level={5} style={{ margin: 0 }}>
            {nav.find((n) => n.key === page)?.label}
          </Title>
          {searching && page !== 'search' && (
            <Tag
              icon={<LoadingOutlined spin />}
              color="processing"
              style={{ marginLeft: 12, cursor: 'pointer' }}
              onClick={() => setPage('search')}
            >
              đang chạy tìm kiếm…
            </Tag>
          )}
        </Header>

        <Content style={{ padding: 24 }}>
          <div style={{ maxWidth: 980, margin: '0 auto' }}>
            {/* SearchPage stays mounted so an in-flight run (spinner, kết quả)
                survives tab switches; only hidden via CSS. */}
            <div style={{ display: page === 'search' ? undefined : 'none' }}>
              <SearchPage
                sources={sources}
                selected={selected}
                onToggle={toggle}
                onSourcesChanged={() => loadSources(false)}
                onBusyChange={setSearching}
              />
            </div>
            {page === 'history' && <HistoryPage />}
            {page === 'settings' && (
              <SettingsPage sources={sources} onChanged={() => loadSources(false)} />
            )}
          </div>
        </Content>
      </Layout>
    </Layout>
  )
}
