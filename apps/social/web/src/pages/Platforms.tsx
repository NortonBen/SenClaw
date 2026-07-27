import { Card, Space, Table, Tooltip, Typography } from 'antd'
import type { Capability, PlatformCaps, Status } from '../api'
import { CapTag } from '../ui'

interface Row extends PlatformCaps {
  platform: string
}

export default function Platforms({ status }: { status: Status | null }) {
  const rows: Row[] = Object.entries(status?.capabilities ?? {}).map(([platform, c]) => ({ platform, ...c }))
  const cap = (v: Capability) => <CapTag value={v} />

  return (
    <Card size="small">
      <Table<Row>
        size="small"
        rowKey="platform"
        dataSource={rows}
        pagination={false}
        locale={{ emptyText: 'Không có dữ liệu.' }}
        columns={[
          {
            title: 'Nền tảng',
            dataIndex: 'platform',
            width: 120,
            render: (v: string) => <b>{v}</b>,
          },
          { title: 'Đăng bài', dataIndex: 'post', width: 110, render: cap },
          { title: 'Nhắn tin', dataIndex: 'dm', width: 110, render: cap },
          { title: 'Tìm kiếm', dataIndex: 'search', width: 120, render: cap },
          { title: 'Duyệt', dataIndex: 'browse', width: 120, render: cap },
          {
            title: 'Ghi chú',
            dataIndex: 'note',
            render: (v: string) => (
              <Tooltip title={v}>
                <Typography.Text type="secondary" className="clamp2" style={{ fontSize: 12 }}>
                  {v}
                </Typography.Text>
              </Tooltip>
            ),
          },
        ]}
      />
      <Space wrap style={{ marginTop: 12, fontSize: 12 }}>
        <CapTag value="official" /> qua API chính thức (trong Rust)
        <CapTag value="replay" /> extension replay request nội bộ
        <CapTag value="page-sign" /> cần trang tự ký (TikTok)
        <CapTag value="dom" /> extension điều khiển giao diện
        <CapTag value="none" /> nền tảng không có — app từ chối ngay
      </Space>
    </Card>
  )
}
