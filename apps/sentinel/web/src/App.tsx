import { useEffect, useState } from 'react'
import { Layout, Tabs, Tag, Space, Button, Tooltip, Segmented, App as AntApp } from 'antd'
import {
  ReloadOutlined,
  SafetyCertificateOutlined,
  WarningOutlined,
  SunOutlined,
  MoonOutlined,
  DesktopOutlined,
} from '@ant-design/icons'
import { api } from './api'
import { useTheme } from './theme'
import type { ThemeMode } from './theme'
import Overview from './Overview'
import Timeline from './Timeline'
import Findings from './Findings'
import Cases from './Cases'
import Rules from './Rules'
import Config from './Config'

export default function App() {
  const [tab, setTab] = useState('overview')
  const [st, setSt] = useState<any>(null)
  const [busy, setBusy] = useState(false)
  const { mode, setMode } = useTheme()
  // Dùng message qua context của AntApp để nó ăn theo chủ đề đang bật, thay vì
  // bản static luôn dựng theo cấu hình mặc định.
  const { message } = AntApp.useApp()

  const load = async () => {
    try {
      setSt(await api.status())
    } catch {
      setSt(null)
    }
  }
  useEffect(() => {
    load()
    const t = setInterval(load, 30000)
    return () => clearInterval(t)
  }, [])

  const rescan = async () => {
    setBusy(true)
    try {
      await api.ingest()
      const r: any = await api.scan()
      message.success(`Đã quét ${r.rules_run} luật — ${r.findings} phát hiện`)
      await load()
    } catch (e: any) {
      message.error('Quét thất bại: ' + e?.message)
    } finally {
      setBusy(false)
    }
  }

  const chainOk = st?.chain?.intact !== false
  const crit = st?.findings?.by_severity?.critical ?? 0
  const high = st?.findings?.by_severity?.high ?? 0

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Layout.Header
        className="sen-header"
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 16,
          paddingInline: 20,
          background: 'transparent',
        }}
      >
        <Space size={10}>
          <SafetyCertificateOutlined style={{ fontSize: 20, color: '#6366f1' }} />
          <span style={{ fontSize: 17, fontWeight: 600 }}>Sentinel</span>
          <span style={{ opacity: 0.55, fontSize: 12 }}>giám sát &amp; điều tra bảo mật AI Agent</span>
        </Space>
        <div style={{ flex: 1 }} />
        <Space size={8}>
          {crit > 0 && <Tag color="red">{crit} nghiêm trọng</Tag>}
          {high > 0 && <Tag color="volcano">{high} cao</Tag>}
          <Tooltip
            title={
              chainOk
                ? 'Chuỗi băm nguyên vẹn — chưa có bản ghi quá khứ nào bị sửa'
                : `Chuỗi băm GÃY tại sự kiện #${st?.chain?.broken_at}`
            }
          >
            <Tag color={chainOk ? 'green' : 'red'} icon={chainOk ? undefined : <WarningOutlined />}>
              {st ? `${st.events} sự kiện` : '…'}
            </Tag>
          </Tooltip>
          <Button icon={<ReloadOutlined />} loading={busy} onClick={rescan}>
            Quét lại
          </Button>
          <Segmented<ThemeMode>
            value={mode}
            onChange={setMode}
            options={[
              { value: 'light', icon: <SunOutlined />, title: 'Sáng' },
              { value: 'dark', icon: <MoonOutlined />, title: 'Tối' },
              { value: 'system', icon: <DesktopOutlined />, title: 'Theo hệ thống' },
            ]}
          />
        </Space>
      </Layout.Header>

      <Layout.Content style={{ padding: 20 }}>
        <Tabs
          activeKey={tab}
          onChange={setTab}
          destroyOnHidden
          items={[
            { key: 'overview', label: 'Tổng quan', children: <Overview onGoTab={setTab} /> },
            { key: 'timeline', label: 'Dòng thời gian', children: <Timeline /> },
            {
              key: 'findings',
              label: `Phát hiện${st?.findings?.open ? ` (${st.findings.open})` : ''}`,
              children: <Findings />,
            },
            { key: 'cases', label: 'Vụ việc', children: <Cases /> },
            { key: 'rules', label: 'Luật', children: <Rules /> },
            { key: 'config', label: 'Cấu hình & Ảnh chụp', children: <Config /> },
          ]}
        />
      </Layout.Content>
    </Layout>
  )
}
