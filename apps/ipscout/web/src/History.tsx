import { useEffect, useMemo, useState } from 'react'
import { Alert, Button, Card, Col, Empty, Row, Select, Space, Table, Tag, Typography } from 'antd'
import { SwapOutlined } from '@ant-design/icons'
import { api, when, type Run } from './api'

const { Text } = Typography

const LAYER_LABEL: Record<string, string> = {
  profile: 'Hồ sơ',
  ports: 'Quét cổng',
  trace: 'Đường đi',
}

/// So hai lần điều tra. Mặc định chọn hai lần **cùng loại** gần nhất — so một
/// lần lập hồ sơ với một lần quét cổng thì mọi cổng đều trông như "vừa mở",
/// đúng kiểu khác biệt giả khiến người đọc mất niềm tin vào cả tính năng.
export default function History({ runs, targetId }: { runs: Run[]; targetId: number | null }) {
  const [from, setFrom] = useState<number | null>(null)
  const [to, setTo] = useState<number | null>(null)
  const [diff, setDiff] = useState<Record<string, any> | null>(null)
  const [busy, setBusy] = useState(false)

  const scans = useMemo(() => runs.filter((r) => r.layer === 'ports' && r.status === 'done'), [runs])

  useEffect(() => {
    setDiff(null)
    setTo(scans[0]?.id ?? null)
    setFrom(scans[1]?.id ?? null)
  }, [targetId, scans.length])

  const compare = async () => {
    if (from == null || to == null) return
    setBusy(true)
    setDiff(await api.diff(from, to))
    setBusy(false)
  }

  const opts = scans.map((r) => ({
    value: r.id,
    label: `#${r.id} · ${when(r.started_at)} · ${r.summary?.open ?? 0} cổng mở`,
  }))

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title="So hai lần quét cổng">
        {scans.length < 2 ? (
          <Text type="secondary">
            Cần ít nhất hai lần quét cổng đã hoàn tất mới so được. Hiện có {scans.length}.
          </Text>
        ) : (
          <Space wrap>
            <Select value={from} onChange={setFrom} options={opts} style={{ minWidth: 280 }} />
            <SwapOutlined />
            <Select value={to} onChange={setTo} options={opts} style={{ minWidth: 280 }} />
            <Button type="primary" loading={busy} onClick={compare}>
              So sánh
            </Button>
          </Space>
        )}

        {diff && (
          <div style={{ marginTop: 16 }}>
            {diff.ip_changed && (
              <Alert
                type="warning"
                showIcon
                style={{ marginBottom: 12 }}
                message={`IP đã đổi: ${diff.ip_from} → ${diff.ip_to}`}
                description="Mục tiêu đang trỏ về một máy chủ khác so với lần trước. Mọi so sánh cổng bên dưới là giữa hai máy khác nhau."
              />
            )}
            <Row gutter={[16, 16]}>
              <Col xs={24} md={8}>
                <Card size="small" title={<Tag color="red">Cổng vừa mở thêm ({diff.opened?.length ?? 0})</Tag>}>
                  {diff.opened?.length ? (
                    diff.opened.map((p: any) => (
                      <div key={p.port}>
                        <Text className="mono" strong>{p.port}</Text>{' '}
                        <Text type="secondary">{p.product ?? p.service ?? ''}</Text>
                      </div>
                    ))
                  ) : (
                    <Text type="secondary">không có</Text>
                  )}
                </Card>
              </Col>
              <Col xs={24} md={8}>
                <Card size="small" title={<Tag color="green">Cổng đã đóng ({diff.closed?.length ?? 0})</Tag>}>
                  {diff.closed?.length ? (
                    diff.closed.map((p: any) => (
                      <div key={p.port}>
                        <Text className="mono" strong>{p.port}</Text>{' '}
                        <Text type="secondary">{p.product ?? p.service ?? ''}</Text>
                      </div>
                    ))
                  ) : (
                    <Text type="secondary">không có</Text>
                  )}
                </Card>
              </Col>
              <Col xs={24} md={8}>
                <Card size="small" title={<Tag color="gold">Đổi phiên bản ({diff.changed?.length ?? 0})</Tag>}>
                  {diff.changed?.length ? (
                    diff.changed.map((c: any) => (
                      <div key={c.port} style={{ fontSize: 12 }}>
                        <Text className="mono" strong>{c.port}</Text>{' '}
                        <Text type="secondary" className="mono">
                          {c.from?.product} {c.from?.version} → {c.to?.product} {c.to?.version}
                        </Text>
                      </div>
                    ))
                  ) : (
                    <Text type="secondary">không có</Text>
                  )}
                </Card>
              </Col>
            </Row>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {diff.unchanged} cổng giữ nguyên. Đổi phiên bản nghĩa là ai đó vừa cập nhật — hoặc vừa
              cài đè lên máy chủ.
            </Text>
          </div>
        )}
      </Card>

      <Card size="small" title="Lịch sử điều tra">
        {runs.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="chưa có lần chạy nào" />
        ) : (
          <div className="scroll-x">
            <Table<Run>
              size="small"
              rowKey="id"
              dataSource={runs}
              pagination={{ pageSize: 12, hideOnSinglePage: true }}
              columns={[
                { title: '#', dataIndex: 'id', width: 60 },
                {
                  title: 'Loại',
                  dataIndex: 'layer',
                  width: 110,
                  render: (l: string) => (
                    <Tag color={l === 'ports' ? 'purple' : 'blue'}>{LAYER_LABEL[l] ?? l}</Tag>
                  ),
                },
                {
                  title: 'Trạng thái',
                  dataIndex: 'status',
                  width: 110,
                  render: (s: string) => (
                    <Tag color={s === 'done' ? 'green' : s === 'failed' ? 'red' : 'gold'}>{s}</Tag>
                  ),
                },
                {
                  title: 'IP',
                  dataIndex: 'ip',
                  width: 150,
                  render: (v: string | null) => <Text className="mono">{v ?? '—'}</Text>,
                },
                {
                  title: 'Kết quả',
                  key: 'r',
                  render: (_, r) =>
                    r.error ? (
                      <Text type="danger">{r.error}</Text>
                    ) : r.layer === 'ports' ? (
                      <Text type="secondary">
                        {r.summary?.open ?? 0}/{r.summary?.scanned ?? 0} cổng mở
                        {r.summary?.os?.os ? ` · ${r.summary.os.os}` : ''}
                      </Text>
                    ) : (
                      <Text type="secondary">
                        {r.summary?.network?.provider ?? r.summary?.asn?.as_name ?? '—'}
                      </Text>
                    ),
                },
                { title: 'Lúc', dataIndex: 'started_at', width: 170, render: when },
              ]}
            />
          </div>
        )}
      </Card>
    </Space>
  )
}
