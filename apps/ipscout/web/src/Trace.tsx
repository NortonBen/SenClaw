import { useState } from 'react'
import {
  Alert, Button, Card, Col, Empty, InputNumber, Row, Space, Statistic, Table, Tag, Tooltip,
  Typography,
} from 'antd'
import { ApartmentOutlined } from '@ant-design/icons'

const { Text, Paragraph } = Typography

interface HopMac {
  addr: string
  iface: string | null
  vendor: string | null
  source: string
}

interface HopNet {
  kind: string
  label: string
  provider: string | null
  fronted: boolean
}

interface Hop {
  ttl: number
  ip: string | null
  rtt_ms: number | null
  asn: number | null
  as_name: string | null
  org: string | null
  ptr: { names: string[]; forward_confirmed: boolean } | null
  network: HopNet | null
  mac: HopMac | null
  note: string | null
}

export default function Trace({
  data,
  running,
  onRun,
}: {
  data: Record<string, any> | null
  running: boolean
  onRun: (maxHops: number) => void
}) {
  const [maxHops, setMaxHops] = useState(30)
  const hops: Hop[] = data?.hops ?? []

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small">
        <Space wrap>
          <Text>TTL trần:</Text>
          <InputNumber value={maxHops} min={5} max={64} onChange={(v) => setMaxHops(Number(v) || 30)} />
          <Button type="primary" icon={<ApartmentOutlined />} loading={running} onClick={() => onRun(maxHops)}>
            Chạy traceroute
          </Button>
        </Space>
        <Paragraph type="secondary" style={{ fontSize: 12, marginTop: 10, marginBottom: 0 }}>
          Traceroute chạy qua binary hệ thống — có ghi log ở phía các router trung gian. Mỗi hop
          được làm giàu bằng ASN + tổ chức + phân loại mạng + PTR. MAC chỉ có với hop{' '}
          <strong>cùng LAN</strong> — hop xa không lấy được, đó là cách IP hoạt động.
        </Paragraph>
      </Card>

      {!data ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="Chưa chạy traceroute. Bấm nút bên trên để bắt đầu."
        />
      ) : (
        <>
          <Row gutter={16}>
            <Col span={6}>
              <Card size="small">
                <Statistic title="Hop trả lời" value={`${data.responded_hops}/${data.total_hops}`} />
              </Card>
            </Col>
            <Col span={6}>
              <Card size="small">
                <Statistic title="Nhà cung cấp mạng (ASN)" value={data.unique_asns} />
              </Card>
            </Col>
            <Col span={12}>
              <Card size="small">
                <Statistic
                  title="CDN đứng trước máy chủ"
                  value={data.cdn_ahead ?? '—'}
                  valueStyle={data.cdn_ahead ? { color: '#fa8c16' } : undefined}
                />
              </Card>
            </Col>
          </Row>

          {data.cdn_ahead && (
            <Alert
              type="warning"
              showIcon
              message={`Traffic của bạn chấm dứt ở biên ${data.cdn_ahead} — không đi thẳng tới máy chủ gốc`}
              description="Một trong các hop trên đường đi thuộc CDN. Từ điểm đó trở đi, kết nối được terminate và tái tạo — mọi kết luận về cổng/OS/dịch vụ ở hop cuối là về biên CDN, không phải về hạ tầng bạn triển khai."
            />
          )}

          <Alert
            type="info"
            showIcon
            style={{ fontSize: 12 }}
            message={data.mac_note}
          />

          <Card size="small" title="Đường đi mạng">
            <div className="scroll-x">
              <Table<Hop>
                size="small"
                rowKey="ttl"
                dataSource={hops}
                pagination={false}
                // Bảng có nhiều cột kèm chuỗi dài (tên AS "CDN77-Datacamp Limited, GB",
                // PTR "unn-89-187-163-220.cdn77.com") — không set width cố định thì antd
                // tự bóp cột khiến chữ vỡ thành một ký tự mỗi dòng ở viewport hẹp.
                scroll={{ x: 900 }}
                columns={[
                  { title: 'TTL', dataIndex: 'ttl', width: 60, render: (t: number) => <Text strong className="mono">{t}</Text> },
                  {
                    title: 'IP',
                    dataIndex: 'ip',
                    width: 170,
                    render: (ip: string | null) =>
                      ip ? <Text className="mono">{ip}</Text> : <Tag>im lặng</Tag>,
                  },
                  {
                    title: 'RTT',
                    dataIndex: 'rtt_ms',
                    width: 90,
                    render: (v: number | null) =>
                      v != null ? <Text className="mono">{v.toFixed(1)}ms</Text> : <Text type="secondary">—</Text>,
                  },
                  {
                    title: 'ASN / Tổ chức',
                    key: 'asn',
                    width: 260,
                    render: (_, r) =>
                      r.asn ? (
                        <Space size={4} wrap>
                          <Tag color="blue">AS{r.asn}</Tag>
                          <Text style={{ fontSize: 12 }}>{r.org ?? r.as_name}</Text>
                        </Space>
                      ) : (
                        <Text type="secondary" style={{ fontSize: 12 }}>{r.note ?? '—'}</Text>
                      ),
                  },
                  {
                    title: 'Loại mạng',
                    key: 'net',
                    width: 170,
                    render: (_, r) =>
                      r.network ? (
                        <Tag color={r.network.fronted ? 'orange' : 'default'} style={{ whiteSpace: 'normal' }}>
                          {r.network.label}
                          {r.network.provider ? ` · ${r.network.provider}` : ''}
                        </Tag>
                      ) : (
                        <Text type="secondary">—</Text>
                      ),
                  },
                  {
                    title: 'PTR',
                    key: 'ptr',
                    width: 260,
                    render: (_, r) =>
                      r.ptr && r.ptr.names.length > 0 ? (
                        <Space size={4} wrap>
                          <Text className="mono" style={{ fontSize: 12, wordBreak: 'break-all' }}>
                            {r.ptr.names[0].replace(/\.$/, '')}
                          </Text>
                          {r.ptr.forward_confirmed && (
                            <Tooltip title="FCrDNS xác nhận: tên tra ngược ra và tra xuôi lại đều khớp IP.">
                              <Tag color="green" style={{ fontSize: 11 }}>xác nhận</Tag>
                            </Tooltip>
                          )}
                        </Space>
                      ) : (
                        <Text type="secondary">—</Text>
                      ),
                  },
                  {
                    title: 'MAC',
                    key: 'mac',
                    width: 200,
                    render: (_, r) =>
                      r.mac ? (
                        <Space size={4} direction="vertical" style={{ gap: 0 }}>
                          <Text className="mono" style={{ fontSize: 12 }}>{r.mac.addr}</Text>
                          <Text type="secondary" style={{ fontSize: 11 }}>
                            {r.mac.iface ?? ''}
                            {r.mac.vendor ? ` · ${r.mac.vendor}` : ''}
                          </Text>
                        </Space>
                      ) : (
                        <Tooltip title={
                          r.ip
                            ? 'MAC chỉ đọc được khi hop cùng LAN với máy chạy app. Hop xa Internet không lấy được — MAC bị router viết lại ở mỗi biên L3.'
                            : 'Hop không trả — không có gì để tra ARP.'
                        }>
                          <Text type="secondary" style={{ fontSize: 12 }}>—</Text>
                        </Tooltip>
                      ),
                  },
                ]}
              />
            </div>
          </Card>
        </>
      )}
    </Space>
  )
}
