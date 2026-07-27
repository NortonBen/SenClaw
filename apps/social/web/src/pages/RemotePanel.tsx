import { useEffect, useState } from 'react'
import { Alert, Badge, Card, Col, Descriptions, Row, Statistic, Tag, Typography } from 'antd'
import { ApiOutlined, ControlOutlined } from '@ant-design/icons'
import { getExtStatus, uptime, type ActionRow, type ExtStatus, type Status } from '../api'

/**
 * "Remote control" panel — makes it obvious WHICH browser extension is remotely
 * driving this app, whether it's live, and what recently went wrong. Combines
 * the app-level `status` with the live extension-bridge stats.
 */
export default function RemotePanel({
  status,
  actions,
}: {
  status: Status | null
  actions: ActionRow[]
}) {
  const [ext, setExt] = useState<ExtStatus | null>(null)

  useEffect(() => {
    const load = async () => setExt(await getExtStatus())
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [])

  const connected = ext?.connected ?? status?.extension_connected ?? false
  const label =
    ext?.label || status?.extension_label || status?.extension_name || 'extension (chưa định danh)'
  const version = ext?.version || status?.extension_version || ''
  const hosts = ext?.hosts_ready ?? status?.extension_hosts_ready ?? []
  const up = ext?.uptime_s ?? status?.extension_uptime_s ?? 0

  // Most recent failures — the whole point of the panel is to surface these.
  const errors = actions.filter((a) => a.status === 'error' || a.status === 'blocked').slice(0, 5)

  return (
    <Card
      size="small"
      style={{ marginBottom: 14 }}
      title={
        <span>
          <ControlOutlined /> Điều khiển từ xa (Remote control)
        </span>
      }
    >
      <Row gutter={[14, 14]}>
        <Col xs={24} md={14}>
          <Descriptions size="small" column={1} bordered>
            <Descriptions.Item
              label={
                <span>
                  <ApiOutlined /> Extension điều khiển
                </span>
              }
            >
              <Badge status={connected ? 'processing' : 'default'} />
              <Typography.Text strong>{connected ? label : 'chưa kết nối'}</Typography.Text>
              {connected && version && <Tag style={{ marginLeft: 8 }}>v{version}</Tag>}
              {ext?.ext_id && (
                <Typography.Text type="secondary" className="mono" style={{ marginLeft: 8, fontSize: 11 }}>
                  {ext.ext_id.slice(0, 12)}…
                </Typography.Text>
              )}
            </Descriptions.Item>
            <Descriptions.Item label="Trạng thái">
              <Tag color={connected ? 'green' : 'red'}>{connected ? 'đang kết nối' : 'ngoại tuyến'}</Tag>
              {connected && <Tag>hoạt động {uptime(up)}</Tag>}
            </Descriptions.Item>
            <Descriptions.Item label="Phiên đăng nhập">
              {hosts.length ? (
                hosts.map((h) => (
                  <Tag color="green" key={h}>
                    {h}
                  </Tag>
                ))
              ) : (
                <Typography.Text type="secondary">chưa thấy phiên nào</Typography.Text>
              )}
            </Descriptions.Item>
          </Descriptions>
        </Col>
        <Col xs={12} md={5}>
          <Statistic title="Số lần kết nối" value={ext?.connects ?? 0} />
        </Col>
        <Col xs={12} md={5}>
          <Statistic
            title="Số lần rớt"
            value={ext?.disconnects ?? 0}
            valueStyle={{ color: (ext?.disconnects ?? 0) > 0 ? '#cf1322' : undefined }}
          />
        </Col>
      </Row>

      {!connected && (
        <Alert
          style={{ marginTop: 12 }}
          type="warning"
          showIcon
          message="Không có extension nào điều khiển app"
          description={
            <>
              Mọi thao tác tìm kiếm / duyệt / nhắn tin đều đi qua extension. Cài thư mục{' '}
              <code>apps/social/extension</code> vào Chrome (Load unpacked) rồi đăng nhập nền tảng.
            </>
          }
        />
      )}

      {errors.length > 0 && (
        <Alert
          style={{ marginTop: 12 }}
          type="error"
          showIcon
          message={`${errors.length} lỗi gần đây từ lệnh điều khiển`}
          description={
            <ul style={{ margin: 0, paddingLeft: 18 }}>
              {errors.map((e, i) => (
                <li key={i}>
                  <Tag>{e.platform}</Tag>
                  <Tag color="red">{e.action}</Tag>
                  <Typography.Text type="secondary">{e.detail || '(không có chi tiết)'}</Typography.Text>
                </li>
              ))}
            </ul>
          }
        />
      )}
    </Card>
  )
}
