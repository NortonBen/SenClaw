import { useEffect, useState } from 'react'
import { Badge, Flex, Segmented, Space, Tabs, Tag, Tooltip, Typography } from 'antd'
import {
  DesktopOutlined,
  DownloadOutlined,
  HistoryOutlined,
  MoonOutlined,
  SettingOutlined,
  SunOutlined,
  UserOutlined,
} from '@ant-design/icons'
import { api, fmtBytes, type Status } from './api'
import { useTheme, type ThemeMode } from './theme'
import DownloadTab from './download'
import HistoryTab from './history'
import ProfileTab from './profile'
import SettingsTab from './settings'

const { Title, Text } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('download')
  const { mode, setMode } = useTheme()

  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 4000)
    return () => clearInterval(t)
  }, [])

  const c = status?.counters

  // Render only the active tab so switching always refetches fresh data.
  const tab = (key: string, node: React.ReactNode) => (active === key ? node : null)

  return (
    <div style={{ maxWidth: 1100, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }} wrap gap={8}>
        <Title level={3} style={{ margin: 0 }}>
          ⬇️ TikTok Downloader <Text type="secondary" style={{ fontSize: 14 }}>— SenClaw</Text>
        </Title>
        <Space wrap>
          {c && c.active + c.queued > 0 ? (
            <Badge status="processing" text={`${c.active} đang tải · ${c.queued} chờ`} />
          ) : (
            <Badge status="success" text="Hàng đợi rảnh" />
          )}
          <Tag color="green">{c?.done ?? 0} đã tải · {fmtBytes(c?.bytes_done)}</Tag>
          {(c?.error ?? 0) > 0 && <Tag color="red">{c?.error} lỗi</Tag>}
          <Tooltip title="Giao diện sáng / tối / theo hệ thống">
            <Segmented
              size="small"
              value={mode}
              onChange={(v) => setMode(v as ThemeMode)}
              options={[
                { value: 'light', icon: <SunOutlined /> },
                { value: 'dark', icon: <MoonOutlined /> },
                { value: 'system', icon: <DesktopOutlined /> },
              ]}
            />
          </Tooltip>
        </Space>
      </Flex>

      <Tabs
        activeKey={active}
        onChange={setActive}
        items={[
          {
            key: 'download',
            label: (
              <span><DownloadOutlined /> Tải xuống</span>
            ),
            children: tab('download', <DownloadTab onChanged={refreshStatus} />),
          },
          {
            key: 'history',
            label: (
              <Badge count={c?.error ?? 0} size="small" offset={[8, -2]}>
                <span><HistoryOutlined /> Lịch sử</span>
              </Badge>
            ),
            children: tab('history', <HistoryTab onChanged={refreshStatus} />),
          },
          {
            key: 'profile',
            label: (
              <span><UserOutlined /> Trang cá nhân</span>
            ),
            children: tab('profile', <ProfileTab onChanged={refreshStatus} />),
          },
          {
            key: 'settings',
            label: (
              <span><SettingOutlined /> Cài đặt</span>
            ),
            children: tab('settings', <SettingsTab />),
          },
        ]}
      />

      <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 16 }}>
        Chỉ tải nội dung công khai cho mục đích lưu trữ cá nhân. Tôn trọng bản quyền và
        quyền riêng tư của tác giả video.
      </Text>
    </div>
  )
}
