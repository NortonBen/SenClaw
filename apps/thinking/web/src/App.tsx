import { useEffect, useState } from 'react'
import { Badge, Flex, Space, Tabs, Tag, Typography } from 'antd'
import { api, type Status } from './api'
import DashboardTab from './dashboard'
import { ActivityTab, ProblemsTab } from './panels'
import ProblemDetail from './detail'

const { Title, Text } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('dashboard')
  // Drawer chi tiết dùng chung cho mọi tab — mở từ dashboard lẫn danh sách.
  const [detailId, setDetailId] = useState<number | null>(null)

  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 20000)
    return () => clearInterval(t)
  }, [])

  // Render only the active tab so switching always refetches fresh data.
  const tab = (key: string, node: React.ReactNode) => (active === key ? node : null)

  return (
    <div style={{ maxWidth: 1200, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          🎩 Tư Duy <Text type="secondary" style={{ fontSize: 14 }}>— 6 Mũ & 5W · SenClaw</Text>
        </Title>
        <Space>
          <Tag color="gold">{status?.open ?? 0} mới</Tag>
          <Tag color="blue">{status?.analyzing ?? 0} đang phân tích</Tag>
          <Tag color="green">{status?.decided ?? 0} đã quyết định</Tag>
          {status && status.attention_count > 0 ? (
            <Badge status="warning" text={`${status.attention_count} cần chú ý`} />
          ) : (
            <Badge status="success" text="Không tồn đọng" />
          )}
        </Space>
      </Flex>

      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          { key: 'dashboard', label: 'Tổng quan', children: tab('dashboard', <DashboardTab onOpen={setDetailId} />) },
          {
            key: 'problems',
            label: (
              <Badge count={status?.attention_count ?? 0} size="small" offset={[8, -2]}>
                Vấn đề
              </Badge>
            ),
            children: tab('problems', <ProblemsTab onOpen={setDetailId} onChange={refreshStatus} />),
          },
          { key: 'activity', label: 'Hoạt động', children: tab('activity', <ActivityTab />) },
        ]}
      />

      {detailId !== null && (
        <ProblemDetail
          id={detailId}
          onClose={() => {
            setDetailId(null)
            refreshStatus()
          }}
        />
      )}
    </div>
  )
}
