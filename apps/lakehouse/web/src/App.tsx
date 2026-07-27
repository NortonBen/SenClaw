import { useState } from 'react'
import { Layout, Menu, Typography } from 'antd'
import {
  ApiOutlined,
  ConsoleSqlOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  DeploymentUnitOutlined,
  HistoryOutlined,
  SettingOutlined,
} from '@ant-design/icons'
import { Overview } from './views/Overview'
import { Datasets } from './views/Datasets'
import { Connections } from './views/Connections'
import { Flows } from './views/Flows'
import { Query } from './views/Query'
import { Runs } from './views/Runs'
import { Settings } from './views/Settings'

type View = 'overview' | 'datasets' | 'connections' | 'flows' | 'query' | 'runs' | 'settings'

const ITEMS = [
  { key: 'overview', icon: <DashboardOutlined />, label: 'Tổng quan' },
  { key: 'datasets', icon: <DatabaseOutlined />, label: 'Datasets' },
  { key: 'connections', icon: <ApiOutlined />, label: 'Kết nối' },
  { key: 'flows', icon: <DeploymentUnitOutlined />, label: 'Flows' },
  { key: 'query', icon: <ConsoleSqlOutlined />, label: 'Truy vấn' },
  { key: 'runs', icon: <HistoryOutlined />, label: 'Runs' },
  { key: 'settings', icon: <SettingOutlined />, label: 'Cài đặt' },
]

export function App() {
  const [view, setView] = useState<View>('overview')

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Layout.Sider breakpoint="lg" collapsedWidth={0} theme="light" width={220}>
        <div style={{ padding: '16px 20px' }}>
          <Typography.Title level={4} style={{ margin: 0 }}>
            🌊 Lakehouse
          </Typography.Title>
        </div>
        <Menu
          mode="inline"
          selectedKeys={[view]}
          onClick={(e) => setView(e.key as View)}
          items={ITEMS}
        />
      </Layout.Sider>
      <Layout>
        <Layout.Content style={{ padding: 24, maxWidth: 1400, width: '100%' }}>
          {view === 'overview' && <Overview onOpenFlows={() => setView('flows')} />}
          {view === 'datasets' && <Datasets />}
          {view === 'connections' && <Connections />}
          {view === 'flows' && <Flows />}
          {view === 'query' && <Query />}
          {view === 'runs' && <Runs />}
          {view === 'settings' && <Settings />}
        </Layout.Content>
      </Layout>
    </Layout>
  )
}
