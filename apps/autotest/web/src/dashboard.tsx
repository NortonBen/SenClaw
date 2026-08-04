import { useEffect, useState } from 'react'
import { Card, Col, Empty, Flex, Row, Statistic, Table, Tag, Tooltip, Typography } from 'antd'
import { api, fmtTime, statusColor, statusLabel, type Run } from './api'

const { Text } = Typography

/** Dải ô vuông màu theo trạng thái run — nhìn một phát ra ngay xu hướng. */
function TrendStrip({ trend }: { trend: any[] }) {
  if (!trend.length) return <Text type="secondary">Chưa có lần chạy nào.</Text>
  return (
    <Flex gap={4} wrap="wrap">
      {trend.map((r) => (
        <Tooltip
          key={r.run_id}
          title={`Run #${r.run_id} · ${statusLabel[r.status] ?? r.status} · ${r.passed}/${r.total} pass · ${fmtTime(r.started_at)}`}
        >
          <div
            style={{
              width: 18,
              height: 26,
              borderRadius: 4,
              background:
                r.status === 'pass' ? '#22c55e' : r.status === 'fail' ? '#ef4444' : r.status === 'error' ? '#f97316' : '#4b5563',
            }}
          />
        </Tooltip>
      ))}
    </Flex>
  )
}

export default function DashboardTab() {
  const [data, setData] = useState<any | null>(null)

  const refresh = () => api.dashboard().then(setData).catch(() => {})
  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 10000)
    return () => clearInterval(t)
  }, [])

  if (!data) return <Empty description="Đang tải…" />
  const rate = data.pass_rate_recent

  return (
    <Flex vertical gap={16}>
      <Row gutter={16}>
        <Col span={4}><Card><Statistic title="Bộ kiểm thử" value={data.suites} /></Card></Col>
        <Col span={4}><Card><Statistic title="Test case" value={data.cases} /></Card></Col>
        <Col span={4}><Card><Statistic title="Run hôm nay" value={data.runs_today} /></Card></Col>
        <Col span={4}>
          <Card>
            <Statistic
              title="Tỷ lệ pass (20 run)"
              value={rate == null ? '—' : Math.round(rate * 100)}
              suffix={rate == null ? '' : '%'}
              valueStyle={{ color: rate == null ? undefined : rate >= 0.9 ? '#22c55e' : rate >= 0.5 ? '#f97316' : '#ef4444' }}
            />
          </Card>
        </Col>
        <Col span={4}><Card><Statistic title="Đang chạy" value={data.running} /></Card></Col>
        <Col span={4}><Card><Statistic title="Lịch đang bật" value={data.schedules_enabled} /></Card></Col>
      </Row>

      <Card title="Xu hướng 20 run gần nhất" size="small">
        <TrendStrip trend={data.trend ?? []} />
      </Card>

      <Row gutter={16}>
        <Col span={12}>
          <Card title="⚠ Test flaky (lúc pass lúc fail)" size="small">
            {(data.flaky ?? []).length === 0 ? (
              <Text type="secondary">Không phát hiện test flaky.</Text>
            ) : (
              <Table
                size="small"
                rowKey="case_id"
                pagination={false}
                dataSource={data.flaky}
                columns={[
                  { title: 'Case', dataIndex: 'name' },
                  { title: 'Suite', dataIndex: 'suite_name' },
                  {
                    title: 'Gần đây (mới → cũ)',
                    dataIndex: 'recent',
                    render: (recent: string[]) => (
                      <Flex gap={3}>
                        {recent.map((s, i) => (
                          <div key={i} style={{ width: 10, height: 16, borderRadius: 2, background: s === 'pass' ? '#22c55e' : '#ef4444' }} />
                        ))}
                      </Flex>
                    ),
                  },
                  { title: 'Đổi trạng thái', dataIndex: 'flips', width: 110 },
                ]}
              />
            )}
          </Card>
        </Col>
        <Col span={12}>
          <Card title="Case hỏng nhiều nhất (30 ngày)" size="small">
            {(data.top_failing ?? []).length === 0 ? (
              <Text type="secondary">Chưa có case nào fail.</Text>
            ) : (
              <Table
                size="small"
                rowKey="case_id"
                pagination={false}
                dataSource={data.top_failing}
                columns={[
                  { title: 'Case', dataIndex: 'name' },
                  { title: 'Số lần fail', dataIndex: 'fail_count', width: 110 },
                  { title: 'Tổng lần chạy', dataIndex: 'total_count', width: 120 },
                ]}
              />
            )}
          </Card>
        </Col>
      </Row>

      <Card title="Run gần đây" size="small">
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={data.recent_runs ?? []}
          columns={[
            { title: '#', dataIndex: 'id', width: 60 },
            { title: 'Đối tượng', dataIndex: 'target' },
            {
              title: 'Trạng thái',
              dataIndex: 'status',
              width: 110,
              render: (s: string) => <Tag color={statusColor[s]}>{statusLabel[s] ?? s}</Tag>,
            },
            {
              title: 'Kết quả',
              width: 160,
              render: (_: any, r: Run) => (
                <Text>
                  <Text style={{ color: '#22c55e' }}>{r.passed}✓</Text>{' '}
                  {r.failed > 0 && <Text style={{ color: '#ef4444' }}>{r.failed}✗ </Text>}
                  {r.errors > 0 && <Text style={{ color: '#f97316' }}>{r.errors}⚠ </Text>}
                  / {r.total}
                </Text>
              ),
            },
            { title: 'Trigger', dataIndex: 'trigger', width: 90 },
            { title: 'Lúc', dataIndex: 'started_at', width: 170, render: fmtTime },
          ]}
        />
      </Card>
    </Flex>
  )
}
