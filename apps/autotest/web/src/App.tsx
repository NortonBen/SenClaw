import { useEffect, useState } from 'react'
import { Badge, Flex, Space, Tabs, Tag, Typography } from 'antd'
import { api, type Status } from './api'
import DashboardTab from './dashboard'
import { ActivityTab, EnvsTab, RunsTab, SchedulesTab, SuitesTab } from './panels'

const { Title, Text } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('dashboard')

  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 15000)
    return () => clearInterval(t)
  }, [])

  // Render only the active tab so switching always refetches fresh data.
  const tab = (key: string, node: React.ReactNode) => (active === key ? node : null)

  const passRate = status?.pass_rate_recent
  return (
    <div style={{ maxWidth: 1150, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          🤖 Tự Động Kiểm Thử <Text type="secondary" style={{ fontSize: 14 }}>— SenClaw</Text>
        </Title>
        <Space>
          {passRate != null && (
            <Tag color={passRate >= 0.9 ? 'green' : passRate >= 0.5 ? 'orange' : 'red'}>
              Pass gần đây: {Math.round(passRate * 100)}%
            </Tag>
          )}
          {status && status.running > 0 ? (
            <Badge status="processing" text={`${status.running} run đang chạy`} />
          ) : (
            <Badge status="success" text="Rảnh" />
          )}
        </Space>
      </Flex>

      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          { key: 'dashboard', label: 'Tổng quan', children: tab('dashboard', <DashboardTab />) },
          {
            key: 'suites',
            label: 'Bộ kiểm thử',
            children: tab('suites', <SuitesTab onChange={refreshStatus} goRuns={() => setActive('runs')} />),
          },
          { key: 'runs', label: 'Lịch sử chạy', children: tab('runs', <RunsTab />) },
          { key: 'envs', label: 'Môi trường', children: tab('envs', <EnvsTab />) },
          { key: 'schedules', label: 'Lịch chạy', children: tab('schedules', <SchedulesTab />) },
          { key: 'activity', label: 'Hoạt động', children: tab('activity', <ActivityTab />) },
        ]}
      />
    </div>
  )
}
