import { useState } from 'react'
import {
  Alert, Button, Card, Col, Empty, Input, Progress, Row, Select, Space, Table, Tag, Tooltip,
  Typography,
} from 'antd'
import { ScanOutlined } from '@ant-design/icons'
import { SEV_COLOR, SEV_LABEL, type PortRow, type Severity } from './api'

const { Text, Paragraph } = Typography

const PROFILES = [
  { value: 'top20', label: 'top20 — 20 cổng hay mở nhất' },
  { value: 'top100', label: 'top100 — 100 cổng phổ biến' },
  { value: 'top1000', label: 'top1000 — 1024 cổng well-known' },
  { value: 'web', label: 'web — cổng web & app' },
  { value: 'db', label: 'db — cơ sở dữ liệu' },
  { value: 'remote', label: 'remote — SSH/RDP/VNC/WinRM' },
  { value: 'mail', label: 'mail — SMTP/POP3/IMAP' },
  { value: 'full', label: 'full — TOÀN BỘ 65535 cổng (chuyên sâu, mất vài phút)' },
]

export default function Ports({
  data,
  scanning,
  onScan,
}: {
  data: Record<string, any> | null
  scanning: boolean
  onScan: (profile: string, ports: string) => void
}) {
  const [profile, setProfile] = useState('top20')
  const [ports, setPorts] = useState('')

  const rows: PortRow[] = data?.ports ?? []
  const os = data?.os

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small">
        <Space wrap>
          <Select
            value={profile}
            onChange={setProfile}
            options={PROFILES}
            style={{ minWidth: 260 }}
          />
          <Tooltip title="Khai trực tiếp thì được ưu tiên hơn hồ sơ. Ví dụ: 22,80,443 hoặc 1-1024. Tối đa 1024 cổng.">
            <Input
              placeholder="hoặc khai cổng: 22,80,443 / 1-1024"
              value={ports}
              onChange={(e) => setPorts(e.target.value)}
              style={{ width: 260 }}
              allowClear
            />
          </Tooltip>
          <Button
            type="primary"
            icon={<ScanOutlined />}
            loading={scanning}
            onClick={() => onScan(profile, ports)}
          >
            Quét cổng
          </Button>
        </Space>
        <Paragraph type="secondary" style={{ fontSize: 12, marginTop: 10, marginBottom: 0 }}>
          Chỉ TCP connect — bắt tay đầy đủ, <strong>có ghi log ở phía máy chủ</strong>. Không quét
          SYN/stealth, không dò mật khẩu, không khai thác. App không kiểm sở hữu — trước khi quét,
          xác nhận đây là hạ tầng của bạn hoặc bạn có uỷ quyền.
        </Paragraph>
      </Card>

      {!data ? (
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa quét lần nào." />
      ) : (
        <>
          {data.fronted && (
            <Alert
              type="warning"
              showIcon
              message={`Kết quả dưới đây mô tả biên của ${data.fronted_by ?? 'CDN'}, không phải máy chủ gốc của bạn`}
            />
          )}

          <Row gutter={[16, 16]}>
            <Col xs={24} lg={14}>
              <Card
                size="small"
                title={
                  <Space size={6}>
                    Cổng mở
                    <Tag color="blue">
                      {data.open}/{data.scanned}
                    </Tag>
                  </Space>
                }
              >
                <div className="scroll-x">
                  <Table<PortRow>
                    size="small"
                    rowKey="port"
                    dataSource={rows}
                    pagination={false}
                    expandable={{
                      rowExpandable: (r) => Boolean(r.banner || r.tls || r.why),
                      expandedRowRender: (r) => (
                        <Space direction="vertical" size={8} style={{ width: '100%' }}>
                          {r.why && <Text>{r.why}</Text>}
                          {r.fix && (
                            <Text type="success">
                              <strong>Cách sửa:</strong> {r.fix}
                            </Text>
                          )}
                          {r.tls && (
                            <div>
                              <Text strong style={{ fontSize: 12 }}>
                                Chứng thư TLS{' '}
                                {r.tls.expired && <Tag color="red">hết hạn</Tag>}
                                {r.tls.self_signed && <Tag color="orange">tự ký</Tag>}
                              </Text>
                              <div className="mono muted" style={{ marginTop: 4 }}>
                                <div>subject: {r.tls.subject}</div>
                                <div>issuer: {r.tls.issuer}</div>
                                <div>hạn: {r.tls.not_before} → {r.tls.not_after}</div>
                                {r.tls.san?.length > 0 && <div>SAN: {r.tls.san.join(', ')}</div>}
                              </div>
                            </div>
                          )}
                          {r.banner && <pre className="banner">{r.banner}</pre>}
                        </Space>
                      ),
                    }}
                    columns={[
                      {
                        title: 'Cổng',
                        dataIndex: 'port',
                        width: 78,
                        render: (p: number) => <Text strong className="mono">{p}</Text>,
                      },
                      {
                        title: 'Dịch vụ',
                        dataIndex: 'service',
                        width: 90,
                        render: (s: string | null) => s ?? <Text type="secondary">—</Text>,
                      },
                      {
                        title: 'Ứng dụng',
                        key: 'product',
                        render: (_, r) =>
                          r.product ? (
                            <>
                              {r.product}{' '}
                              {r.version && <Text type="secondary" className="mono">{r.version}</Text>}
                            </>
                          ) : (
                            <Text type="secondary">chưa nhận dạng</Text>
                          ),
                      },
                      {
                        title: 'Mức',
                        dataIndex: 'severity',
                        width: 120,
                        render: (s: Severity) => <Tag color={SEV_COLOR[s]}>{SEV_LABEL[s]}</Tag>,
                      },
                    ]}
                  />
                </div>
              </Card>
            </Col>

            <Col xs={24} lg={10}>
              <Card size="small" title="Hệ điều hành (suy luận)">
                {os?.os ? (
                  <Space direction="vertical" size={10} style={{ width: '100%' }}>
                    <Space align="baseline">
                      <Text strong style={{ fontSize: 18 }}>
                        {os.os}
                      </Text>
                      <Tag color={os.confidence >= 80 ? 'green' : os.confidence >= 50 ? 'gold' : 'orange'}>
                        {os.confidence}% tin cậy
                      </Tag>
                    </Space>
                    <Progress
                      percent={os.confidence}
                      showInfo={false}
                      size="small"
                      strokeColor={os.confidence >= 80 ? '#52c41a' : '#faad14'}
                    />

                    {os.conflicts?.length > 0 && (
                      <Alert
                        type="warning"
                        showIcon
                        style={{ fontSize: 12 }}
                        message={`Bằng chứng còn chỉ về: ${os.conflicts.join(', ')}`}
                      />
                    )}

                    <div>
                      <Text strong style={{ fontSize: 12 }}>
                        Bằng chứng đã dùng
                      </Text>
                      {(os.evidence ?? []).map((e: any, i: number) => (
                        <div key={i} style={{ marginTop: 6 }}>
                          <Space size={6} align="start">
                            <Tag color="blue" style={{ minWidth: 44, textAlign: 'center' }}>
                              {e.weight}
                            </Tag>
                            <Text style={{ fontSize: 12 }}>
                              <strong>{e.os}</strong> — <Text type="secondary">{e.from}</Text>
                            </Text>
                          </Space>
                        </div>
                      ))}
                    </div>

                    <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
                      {os.note}
                    </Paragraph>
                  </Space>
                ) : (
                  <Space direction="vertical" size={8}>
                    <Text type="secondary">Không kết luận được.</Text>
                    <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
                      {os?.note ??
                        'Không có bằng chứng nào. Với máy chủ được làm cứng đúng cách thì đây là kết quả mong đợi — gỡ nhãn phân phối khỏi banner và giấu header Server là biện pháp đúng.'}
                    </Paragraph>
                  </Space>
                )}

                {os?.not_covered && (
                  <div style={{ marginTop: 14 }}>
                    <Text strong style={{ fontSize: 12 }}>
                      Phương pháp này KHÔNG thấy được
                    </Text>
                    <ul className="muted" style={{ fontSize: 12, paddingLeft: 18, margin: '6px 0 0' }}>
                      {os.not_covered.map((x: string, i: number) => (
                        <li key={i}>{x}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </Card>
            </Col>
          </Row>
        </>
      )}
    </Space>
  )
}
