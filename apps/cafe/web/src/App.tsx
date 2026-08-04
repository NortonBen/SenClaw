import { useEffect, useState } from 'react'
import { Badge, Button, Flex, Tabs, Tag, Typography } from 'antd'
import { MoonOutlined, SunOutlined } from '@ant-design/icons'
import { api, fmtMoney, type Status } from './api'
import { useThemeMode } from './theme'
import { DashboardTab } from './dashboard'
import { SalesTab } from './sales'
import { MenuTab } from './menu'
import { InventoryTab } from './inventory'
import { PurchasesTab } from './purchases'
import { ReportsTab } from './reports'
import { ForecastTab } from './forecast'

const { Title } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('overview')
  const { mode, toggle } = useThemeMode()

  const refresh = () => {
    api.status().then(setStatus).catch(() => setStatus(null))
  }
  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 20_000)
    return () => clearInterval(t)
  }, [])

  // Render only the active tab so switching always refetches fresh data.
  const tab = (key: string, node: React.ReactNode) => (active === key ? node : null)

  return (
    <div style={{ padding: '12px 16px 40px', maxWidth: 1280, margin: '0 auto' }}>
      <Flex align="center" gap={12} style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          ☕ Quán Cafe
        </Title>
        {status && (
          <>
            <Tag color="green">hôm nay {fmtMoney(status.today_revenue)}</Tag>
            {status.low_stock_count > 0 && (
              <Badge count={status.low_stock_count} title="Nguyên liệu sắp hết">
                <Tag color="red">sắp hết nguyên liệu</Tag>
              </Badge>
            )}
          </>
        )}
        <Button
          style={{ marginLeft: 'auto' }}
          icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
          onClick={toggle}
          title={mode === 'dark' ? 'Chuyển giao diện sáng' : 'Chuyển giao diện tối'}
        />
      </Flex>
      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          { key: 'overview', label: 'Tổng quan', children: tab('overview', <DashboardTab />) },
          { key: 'sales', label: 'Bán hàng', children: tab('sales', <SalesTab onChange={refresh} />) },
          { key: 'menu', label: 'Thực đơn & công thức', children: tab('menu', <MenuTab onChange={refresh} />) },
          { key: 'inventory', label: 'Kho nguyên liệu', children: tab('inventory', <InventoryTab onChange={refresh} />) },
          { key: 'purchases', label: 'Nhập hàng', children: tab('purchases', <PurchasesTab onChange={refresh} />) },
          { key: 'reports', label: 'Báo cáo', children: tab('reports', <ReportsTab />) },
          { key: 'forecast', label: 'Dự đoán', children: tab('forecast', <ForecastTab />) },
        ]}
      />
    </div>
  )
}
