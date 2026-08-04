import { useEffect, useState } from 'react'
import { Badge, Flex, Space, Tabs, Tag, Typography } from 'antd'
import { api, fmtMoney, type Status } from './api'
import DashboardTab from './dashboard'
import GoalsTab from './goals'
import UsageTab from './usage'
import { ActivityTab, AllocTab, ScheduleTab, SourcesTab, TxTab } from './panels'

const { Title, Text } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('dashboard')

  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 20000)
    return () => clearInterval(t)
  }, [])

  // Render only the active tab so switching always refetches fresh data.
  const tab = (key: string, node: React.ReactNode) => (active === key ? node : null)

  return (
    <div style={{ maxWidth: 1100, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          💰 Nguồn Vốn <Text type="secondary" style={{ fontSize: 14 }}>— SenClaw</Text>
        </Title>
        <Space>
          <Tag color="green">Dư nợ: {fmtMoney(status?.debt_outstanding ?? 0)}</Tag>
          {status && status.overdue_count > 0 ? (
            <Badge status="error" text={`${status.overdue_count} kỳ quá hạn`} />
          ) : (
            <Badge status="success" text="Không có kỳ quá hạn" />
          )}
        </Space>
      </Flex>

      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          { key: 'dashboard', label: 'Tổng quan', children: tab('dashboard', <DashboardTab />) },
          { key: 'sources', label: 'Nguồn vốn', children: tab('sources', <SourcesTab onChange={refreshStatus} />) },
          { key: 'tx', label: 'Giao dịch', children: tab('tx', <TxTab onChange={refreshStatus} />) },
          {
            key: 'schedule',
            label: (
              <Badge count={status?.overdue_count ?? 0} size="small" offset={[8, -2]}>
                Lịch trả nợ
              </Badge>
            ),
            children: tab('schedule', <ScheduleTab onChange={refreshStatus} />),
          },
          { key: 'alloc', label: 'Phân bổ vốn', children: tab('alloc', <AllocTab />) },
          { key: 'goals', label: 'Mục tiêu', children: tab('goals', <GoalsTab onChange={refreshStatus} />) },
          { key: 'usage', label: 'Sử dụng vốn', children: tab('usage', <UsageTab />) },
          { key: 'activity', label: 'Hoạt động', children: tab('activity', <ActivityTab />) },
        ]}
      />
    </div>
  )
}
