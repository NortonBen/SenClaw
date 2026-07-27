import { useEffect, useState } from 'react'
import { Alert, App, Card, Col, Descriptions, Row, Select, Tag, Typography } from 'antd'
import { getExtStatus, mutate, type ExtStatus, type Status } from '../api'

export default function Settings({ status, onChanged }: { status: Status | null; onChanged: () => void }) {
  const { message } = App.useApp()
  const [ext, setExt] = useState<ExtStatus | null>(null)

  useEffect(() => {
    const load = async () => setExt(await getExtStatus())
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [])

  const setMode = async (v: string) => {
    const r = await mutate('/api/settings', 'PUT', { autonomy: v })
    if (r.ok) message.success(`Đã chuyển chế độ: ${v}`)
    else message.error(r.error ?? 'Lỗi lưu chế độ')
    onChanged()
  }

  return (
    <>
      <Row gutter={[14, 14]}>
        <Col xs={24} lg={12}>
          <Card size="small" title="Chế độ tự chủ">
            <Select
              style={{ width: '100%' }}
              value={status?.autonomy}
              onChange={setMode}
              options={[
                { value: 'observe', label: 'observe — chỉ đọc' },
                { value: 'draft', label: 'draft — tạo nháp chờ duyệt (khuyến nghị)' },
                { value: 'live', label: 'live — gửi ngay' },
              ]}
            />
            <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginTop: 10, marginBottom: 0 }}>
              Ở <code>draft</code>, mọi bài đăng và tin trả lời trở thành nháp để Sếp duyệt.{' '}
              <b>live</b> bỏ qua bước duyệt tay — chỉ bật khi thật sự cần.
            </Typography.Paragraph>
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card size="small" title="Chrome extension">
            <Tag color={ext?.connected ? 'green' : 'red'}>
              {ext?.connected ? 'đã kết nối' : 'chưa kết nối'}
            </Tag>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              uptime {ext?.uptime_s ?? 0}s · kết nối {ext?.connects ?? 0} lần · rớt {ext?.disconnects ?? 0}
            </Typography.Text>
            <Typography.Paragraph style={{ fontSize: 12, marginTop: 8, marginBottom: 4 }}>
              Phiên sẵn sàng: {ext?.hosts_ready?.join(', ') || '—'}
            </Typography.Paragraph>
            <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
              Cài: <code>chrome://extensions</code> → Developer mode → Load unpacked → chọn{' '}
              <code>apps/social/extension</code>, rồi đăng nhập nền tảng trong chính Chrome đó.
            </Typography.Paragraph>
          </Card>
        </Col>
      </Row>

      <Card size="small" title="Hệ thống" style={{ marginTop: 14 }}>
        <Descriptions size="small" column={{ xs: 1, sm: 2 }} bordered>
          <Descriptions.Item label="Nền tảng hỗ trợ">{(status?.platforms ?? []).join(', ')}</Descriptions.Item>
          <Descriptions.Item label="Cổng ứng dụng">
            <span className="mono">{status?.port ?? '—'}</span>
          </Descriptions.Item>
          <Descriptions.Item label="Cổng cầu extension (WS)">
            <span className="mono">{status?.ext_ws_port ?? '—'}</span>
          </Descriptions.Item>
          <Descriptions.Item label="Tài khoản / nháp chờ">
            {status?.accounts ?? 0} / {status?.drafts_pending ?? 0}
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Alert
        style={{ marginTop: 14 }}
        type="warning"
        showIcon
        message="Ranh giới an toàn"
        description={
          <ul style={{ margin: 0, paddingLeft: 18 }}>
            <li>
              <b>Không có cách nào bảo đảm 100% không bị nền tảng chặn.</b> App chỉ giảm rủi ro: nhịp người,
              hạn mức ngày, và duyệt tay.
            </li>
            <li>
              Đăng bài ưu tiên API chính thức (FB Page, X, Threads đã nối thật). Tìm kiếm/duyệt/nhắn tin đi qua
              extension bằng phiên đăng nhập thật.
            </li>
            <li>Nhắn tin chỉ để trả lời — không nhắn nguội, không gửi hàng loạt.</li>
            <li>Token phiên do extension giữ tại máy; app chỉ biết "có phiên hay không".</li>
          </ul>
        }
      />
    </>
  )
}
