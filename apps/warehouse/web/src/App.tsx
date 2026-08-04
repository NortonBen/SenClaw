import { useEffect, useState } from 'react'
import { Badge, Flex, Space, Tabs, Tag, Typography } from 'antd'
import { api, fmtMoney, type Status } from './api'
import DashboardTab from './dashboard'
import InsightTab from './insight'
import { ActivityTab, MovesTab, PartnersTab, ProductsTab, StockCardTab, WarehousesTab } from './panels'

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
    <div style={{ maxWidth: 1200, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          📦 Kho Hàng <Text type="secondary" style={{ fontSize: 14 }}>— SenClaw</Text>
        </Title>
        <Space>
          <Tag color="gold">Giá trị tồn: {fmtMoney(status?.stock_value ?? 0)}</Tag>
          {status && status.low_stock_count > 0 ? (
            <Badge status="error" text={`${status.low_stock_count} mặt hàng sắp hết`} />
          ) : (
            <Badge status="success" text="Tồn kho ổn" />
          )}
        </Space>
      </Flex>

      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          { key: 'dashboard', label: 'Tổng quan', children: tab('dashboard', <DashboardTab />) },
          {
            key: 'products',
            label: (
              <Badge count={status?.low_stock_count ?? 0} size="small" offset={[8, -2]}>
                Sản phẩm
              </Badge>
            ),
            children: tab('products', <ProductsTab onChange={refreshStatus} />),
          },
          { key: 'moves', label: 'Phiếu kho', children: tab('moves', <MovesTab onChange={refreshStatus} />) },
          { key: 'insight', label: 'Phân tích SP', children: tab('insight', <InsightTab />) },
          { key: 'card', label: 'Thẻ kho', children: tab('card', <StockCardTab />) },
          { key: 'warehouses', label: 'Kho', children: tab('warehouses', <WarehousesTab onChange={refreshStatus} />) },
          { key: 'partners', label: 'Đối tác', children: tab('partners', <PartnersTab />) },
          { key: 'activity', label: 'Hoạt động', children: tab('activity', <ActivityTab />) },
        ]}
      />
    </div>
  )
}
