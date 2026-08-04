import { Alert, Card, Col, Empty, Row, Space, Tag, Tooltip, Typography } from 'antd'
import { CONF_COLOR } from './api'

const { Text, Paragraph } = Typography

function KV({ rows }: { rows: [string, React.ReactNode][] }) {
  const shown = rows.filter(([, v]) => v !== null && v !== undefined && v !== '')
  if (shown.length === 0) return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="không có dữ liệu" />
  return (
    <dl className="kv">
      {shown.map(([k, v]) => (
        <div key={k} style={{ display: 'contents' }}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  )
}

const list = (a: unknown): string | null =>
  Array.isArray(a) && a.length > 0 ? a.map(String).map((s) => s.replace(/\.$/, '')).join(', ') : null

export default function Profile({ data }: { data: Record<string, any> | null }) {
  if (!data) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="Chưa có hồ sơ. Bấm “Lập hồ sơ” để bắt đầu — bước này không gửi gói tin nào tới mục tiêu."
      />
    )
  }

  if (data.private) {
    return <Alert type="info" showIcon message="Địa chỉ nội bộ" description={data.note} />
  }

  const asn = data.asn ?? {}
  const reg = data.registry ?? {}
  const geo = data.geo ?? {}
  const net = data.network ?? {}
  const ptr = data.ptr ?? {}
  const dns = data.dns
  const rep = data.reputation ?? {}
  const conf = geo.confidence ?? {}

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {/* Cảnh báo CDN đứng trên cùng: nó quyết định mọi thẻ bên dưới đang nói về ai. */}
      {net.fronted && (
        <Alert
          type="warning"
          showIcon
          message={`IP này là biên của ${net.provider ?? 'CDN'}, không phải máy chủ gốc`}
          description={
            <>
              {net.reason?.replace(/\*\*/g, '')}
              <br />
              <Text type="secondary">
                Nghĩa là: cổng mở, phiên bản dịch vụ và hệ điều hành đọc được ở tab bên cạnh đều mô tả{' '}
                {net.provider ?? 'CDN'} — không phải hạ tầng của bạn.
              </Text>
            </>
          }
        />
      )}

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={12}>
          <Card size="small" title="Mạng — traffic đi qua đâu" className={net.fronted ? 'fronted' : undefined}>
            <KV
              rows={[
                [
                  'Phân loại',
                  <Space size={4} wrap>
                    <Tag color={net.fronted ? 'orange' : 'blue'}>{net.label ?? '—'}</Tag>
                    {net.provider && <Text strong>{net.provider}</Text>}
                    {net.anycast && (
                      <Tooltip title="Cùng một IP được quảng bá từ nhiều nơi trên thế giới — vị trí địa lý mất ý nghĩa.">
                        <Tag color="purple">anycast</Tag>
                      </Tooltip>
                    )}
                  </Space>,
                ],
                ['ASN', asn.asn ? <Text className="mono">AS{asn.asn}</Text> : null],
                ['Tên AS', asn.as_name],
                ['Dải quảng bá', asn.prefix ? <Text className="mono">{asn.prefix}</Text> : null],
                ['RIR', asn.rir ? `${asn.rir.toUpperCase()} · cấp ${asn.allocated ?? '?'}` : null],
              ]}
            />
          </Card>
        </Col>

        <Col xs={24} lg={12}>
          <Card size="small" title="Đăng ký (RDAP) — IP này của ai">
            <KV
              rows={[
                ['Tổ chức', reg.org],
                ['Tên dải', reg.net_name],
                ['CIDR', reg.cidr ? <Text className="mono">{reg.cidr}</Text> : null],
                ['Khoảng', reg.range ? <Text className="mono">{reg.range}</Text> : null],
                ['Quốc gia đăng ký', reg.country],
                ['Loại cấp phát', reg.type],
                ['Đăng ký', reg.registered?.slice(0, 10)],
                ['Sửa lần cuối', reg.last_changed?.slice(0, 10)],
                [
                  'Liên hệ abuse',
                  reg.abuse_email ? (
                    <Text copyable className="mono">
                      {reg.abuse_email}
                    </Text>
                  ) : null,
                ],
              ]}
            />
          </Card>
        </Col>

        <Col xs={24} lg={12}>
          <Card
            size="small"
            title={
              <Space size={6}>
                Vị trí địa lý
                <Tag color={CONF_COLOR[conf.country] ?? 'default'}>quốc gia: {conf.country ?? '—'}</Tag>
                <Tag color={CONF_COLOR[conf.city] ?? 'default'}>thành phố: {conf.city ?? '—'}</Tag>
              </Space>
            }
          >
            <KV
              rows={[
                ['Quốc gia', geo.country ? `${geo.country} (${geo.country_code})` : null],
                ['Vùng', geo.region],
                ['Thành phố', geo.city],
                ['Múi giờ', geo.timezone],
                [
                  'Toạ độ',
                  geo.lat != null ? (
                    <Text className="mono">
                      {geo.lat}, {geo.lon}
                    </Text>
                  ) : null,
                ],
                ['ISP', geo.isp],
                [
                  'Đối chiếu',
                  conf.sources_agree === true ? (
                    <Tag color="green">hai nguồn khớp nhau</Tag>
                  ) : conf.sources_agree === false ? (
                    <Tag color="red">hai nguồn KHÔNG khớp</Tag>
                  ) : (
                    <Tag>chỉ một nguồn</Tag>
                  ),
                ],
              ]}
            />
            {conf.note && (
              <Paragraph type="secondary" style={{ fontSize: 12, marginTop: 12, marginBottom: 0 }}>
                {conf.note}
              </Paragraph>
            )}
          </Card>
        </Col>

        <Col xs={24} lg={12}>
          <Card size="small" title="Tên ngược & DNS">
            <KV
              rows={[
                [
                  'PTR',
                  ptr.lookup_ok === false ? (
                    <Tag>không tra được</Tag>
                  ) : list(ptr.names) ? (
                    <Space size={4} wrap>
                      <Text className="mono">{list(ptr.names)}</Text>
                      {ptr.forward_confirmed ? (
                        <Tooltip title="Tra ngược ra tên, rồi tra xuôi tên đó về đúng IP này (FCrDNS). Hai chiều cùng xác nhận.">
                          <Tag color="green">đã xác nhận xuôi</Tag>
                        </Tooltip>
                      ) : (
                        <Tooltip title="PTR do chủ dải IP tự đặt nên một mình nó không chứng minh gì. Tên này không tra xuôi về lại IP.">
                          <Tag color="orange">chưa xác nhận</Tag>
                        </Tooltip>
                      )}
                    </Space>
                  ) : (
                    <Tag>không có</Tag>
                  ),
                ],
                ['A/AAAA', dns ? list(dns.a) : null],
                ['NS', dns ? list(dns.ns) : null],
                ['MX', dns ? list(dns.mx) : null],
                ['CNAME', dns ? list(dns.cname) : null],
              ]}
            />
            {!dns && (
              <Paragraph type="secondary" style={{ fontSize: 12, marginTop: 12, marginBottom: 0 }}>
                Mục tiêu là IP trần nên không có bản ghi DNS xuôi để tra.
              </Paragraph>
            )}
          </Card>
        </Col>

        <Col xs={24}>
          <Card
            size="small"
            title={
              <Space size={6}>
                Danh sách chặn thư rác
                {rep.listed_count > 0 ? (
                  <Tag color="red">có trong {rep.listed_count}</Tag>
                ) : (
                  <Tag color="green">không có trong danh sách nào</Tag>
                )}
                {rep.unknown_count > 0 && <Tag color="gold">{rep.unknown_count} không tra được</Tag>}
              </Space>
            }
          >
            <Space direction="vertical" size={8} style={{ width: '100%' }}>
              {(rep.results ?? []).map((r: any) => (
                <div key={r.zone}>
                  <Space size={6} align="start">
                    <Tag
                      color={
                        r.status === 'listed' ? 'red' : r.status === 'clean' ? 'green' : 'gold'
                      }
                      style={{ minWidth: 76, textAlign: 'center' }}
                    >
                      {r.status === 'listed' ? 'CÓ' : r.status === 'clean' ? 'sạch' : 'chưa rõ'}
                    </Tag>
                    <Text className="mono" style={{ minWidth: 180, display: 'inline-block' }}>
                      {r.zone}
                    </Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {r.meaning}
                    </Text>
                  </Space>
                </div>
              ))}
            </Space>
          </Card>
        </Col>
      </Row>
    </Space>
  )
}
