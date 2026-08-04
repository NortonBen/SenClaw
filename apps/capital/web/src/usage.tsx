import { useEffect, useState } from 'react'
import { Alert, Card, Col, Empty, Flex, List, Progress, Row, Space, Spin, Statistic, Table, Tag, Tooltip, Typography } from 'antd'
import { FundOutlined, StarOutlined } from '@ant-design/icons'
import { api, fmtMoney, SOURCE_KIND_LABELS, type SourceRating, type Usage } from './api'

const { Text } = Typography

function gradeColor(grade: string): string {
  return grade === 'A' ? '#10b981' : grade === 'B' ? '#1677ff' : grade === 'C' ? '#fa8c16' : '#f5222d'
}

export default function UsageTab() {
  const [usage, setUsage] = useState<Usage | null>(null)
  const [ratings, setRatings] = useState<SourceRating[] | null>(null)
  const [wavg, setWavg] = useState(0)

  useEffect(() => {
    api.usage().then(setUsage).catch(() => {})
    api
      .ratings()
      .then((r) => {
        setRatings(r.ratings)
        setWavg(r.weighted_debt_rate)
      })
      .catch(() => {})
  }, [])

  if (!usage || ratings === null) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {usage.signals.map((s, i) => (
        <Alert
          key={i}
          type={s.severity === 'crit' ? 'error' : s.severity === 'warn' ? 'warning' : 'success'}
          showIcon
          message={s.title}
          description={s.detail}
        />
      ))}

      <Card
        size="small"
        title={
          <Space>
            <FundOutlined style={{ color: '#10b981' }} />
            Tiền đã dùng vào đâu
          </Space>
        }
      >
        <Row gutter={[12, 12]} style={{ marginBottom: 12 }}>
          <Col xs={8}>
            <Statistic title="Tổng đã giải ngân" value={fmtMoney(usage.total_disbursed)} valueStyle={{ fontSize: 18 }} />
          </Col>
          <Col xs={8}>
            <Statistic title="Đã gắn mục đích" value={fmtMoney(usage.allocated)} valueStyle={{ fontSize: 18, color: '#10b981' }} />
          </Col>
          <Col xs={8}>
            <Statistic
              title="Chưa phân loại"
              value={fmtMoney(usage.unallocated)}
              valueStyle={{ fontSize: 18, color: usage.unallocated_pct > 50 ? '#fa8c16' : undefined }}
            />
          </Col>
        </Row>
        {!usage.by_allocation.length ? (
          <Empty description="Chưa có phân bổ — tạo phân bổ ở tab Phân bổ vốn rồi gắn khi giải ngân" image={Empty.PRESENTED_IMAGE_SIMPLE} />
        ) : (
          <Table
            size="small"
            rowKey="id"
            pagination={false}
            dataSource={usage.by_allocation}
            columns={[
              { title: 'Mục đích / dự án', dataIndex: 'name' },
              { title: 'Đã rót', dataIndex: 'used', align: 'right' as const, render: (v: number) => fmtMoney(v) },
              {
                title: '% tổng giải ngân',
                dataIndex: 'share_pct',
                width: 200,
                render: (v: number) => <Progress percent={Math.round(v)} size="small" />,
              },
              {
                title: 'So ngân sách',
                dataIndex: 'budget_used_pct',
                align: 'right' as const,
                render: (v: number | null, r) =>
                  v === null ? (
                    <Text type="secondary">—</Text>
                  ) : (
                    <Text style={{ color: r.over_budget ? '#f5222d' : undefined }}>
                      {Math.round(v)}%{r.over_budget ? ' (vượt)' : ''}
                    </Text>
                  ),
              },
            ]}
          />
        )}
      </Card>

      <Card title="Mức tận dụng từng nguồn" size="small">
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={usage.by_source}
          columns={[
            {
              title: 'Nguồn',
              dataIndex: 'name',
              render: (v: string, r) => (
                <Space direction="vertical" size={0}>
                  <Text strong>{v}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>{SOURCE_KIND_LABELS[r.kind] ?? r.kind}</Text>
                </Space>
              ),
            },
            { title: 'Cam kết', dataIndex: 'committed', align: 'right' as const, render: (v: number) => fmtMoney(v) },
            { title: 'Đã rút', dataIndex: 'disbursed', align: 'right' as const, render: (v: number) => fmtMoney(v) },
            {
              title: 'Tận dụng',
              dataIndex: 'utilization_pct',
              width: 180,
              render: (v: number) => <Progress percent={Math.min(Math.round(v), 100)} size="small" />,
            },
            { title: 'Nhàn rỗi / còn rút được', dataIndex: 'idle', align: 'right' as const, render: (v: number) => fmtMoney(v) },
          ]}
        />
      </Card>

      <Card
        size="small"
        title={
          <Space>
            <StarOutlined style={{ color: '#10b981' }} />
            Đánh giá từng nguồn tiền
            {wavg > 0 && <Text type="secondary" style={{ fontSize: 12 }}>(mặt bằng lãi suất sổ: {wavg}%/năm)</Text>}
          </Space>
        }
      >
        {!ratings.length ? (
          <Empty description="Chưa có nguồn nào đang hoạt động" image={Empty.PRESENTED_IMAGE_SIMPLE} />
        ) : (
          <Row gutter={[12, 12]}>
            {ratings.map((r) => (
              <Col xs={24} md={12} key={r.id}>
                <Card size="small">
                  <Flex gap={12} align="flex-start">
                    <Tooltip title={`Điểm ${r.score}/100`}>
                      <Progress
                        type="circle"
                        size={64}
                        percent={r.score}
                        strokeColor={gradeColor(r.grade)}
                        format={() => (
                          <span style={{ fontSize: 22, fontWeight: 700, color: gradeColor(r.grade) }}>{r.grade}</span>
                        )}
                      />
                    </Tooltip>
                    <Space direction="vertical" size={2} style={{ flex: 1 }}>
                      <Space wrap>
                        <Text strong>{r.name}</Text>
                        <Tag>{SOURCE_KIND_LABELS[r.kind] ?? r.kind}</Tag>
                        {r.is_debt && r.outstanding > 0 && (
                          <Text type="secondary" style={{ fontSize: 12 }}>dư {fmtMoney(r.outstanding)}</Text>
                        )}
                      </Space>
                      <Text style={{ fontSize: 13 }}>{r.verdict}</Text>
                      <List
                        size="small"
                        dataSource={r.factors}
                        renderItem={(f) => (
                          <List.Item style={{ padding: '2px 0', border: 'none' }}>
                            <Text
                              type="secondary"
                              style={{ fontSize: 12, color: f.impact === '-' ? '#f5222d' : f.impact === '+' ? '#10b981' : undefined }}
                            >
                              {f.impact === '-' ? '▼' : f.impact === '+' ? '▲' : '•'} {f.text}
                            </Text>
                          </List.Item>
                        )}
                      />
                    </Space>
                  </Flex>
                </Card>
              </Col>
            ))}
          </Row>
        )}
      </Card>
    </Space>
  )
}
