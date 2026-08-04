import { useEffect, useState } from 'react'
import { Card, Col, Empty, Progress, Row, Spin, Statistic, Table, Tag, Typography } from 'antd'
import {
  api,
  fmtTime,
  PRIORITY_COLORS,
  PRIORITY_LABELS,
  STATUS_COLORS,
  STATUS_LABELS,
  type Dashboard,
  type Problem,
} from './api'

const { Text } = Typography

export default function DashboardTab({ onOpen }: { onOpen: (id: number) => void }) {
  const [dash, setDash] = useState<Dashboard | null>(null)

  useEffect(() => {
    api.dashboard().then(setDash).catch(() => {})
  }, [])

  if (!dash) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  const problemCols = [
    {
      title: 'Vấn đề',
      dataIndex: 'title',
      render: (t: string, r: Problem) => (
        <a onClick={() => onOpen(r.id)}>
          {t} {r.priority === 'high' && <Tag color="red">Cao</Tag>}
        </a>
      ),
    },
    {
      title: 'Trạng thái',
      dataIndex: 'status',
      width: 130,
      render: (s: string) => <Tag color={STATUS_COLORS[s]}>{STATUS_LABELS[s] ?? s}</Tag>,
    },
    {
      title: 'Phân tích',
      dataIndex: 'completeness',
      width: 150,
      render: (c: number) => <Progress percent={c} size="small" />,
    },
    { title: 'Giải pháp', dataIndex: 'solution_count', width: 90, align: 'center' as const },
    { title: 'Cập nhật', dataIndex: 'updated_at', width: 150, render: fmtTime },
  ]

  return (
    <div>
      <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title="Tổng vấn đề" value={dash.problems_total} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title="Mới" value={dash.by_status.open} valueStyle={{ color: '#f59e0b' }} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title="Đang phân tích" value={dash.by_status.analyzing} valueStyle={{ color: '#3b82f6' }} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title="Đã quyết định" value={dash.by_status.decided} valueStyle={{ color: '#22c55e' }} /></Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={14}>
          <Card size="small" title="Vấn đề gần đây">
            {dash.recent.length === 0 ? (
              <Empty description="Chưa có vấn đề nào — tạo vấn đề đầu tiên ở tab Vấn đề" />
            ) : (
              <Table rowKey="id" size="small" pagination={false} columns={problemCols} dataSource={dash.recent} />
            )}
          </Card>
        </Col>
        <Col xs={24} lg={10}>
          <Card size="small" title="⚠️ Cần chú ý (phân tích dở dang)" style={{ marginBottom: 16 }}>
            {dash.attention.length === 0 ? (
              <Empty description="Không có vấn đề tồn đọng" image={Empty.PRESENTED_IMAGE_SIMPLE} />
            ) : (
              dash.attention.map((p) => (
                <div key={p.id} style={{ marginBottom: 8 }}>
                  <a onClick={() => onOpen(p.id)}>{p.title}</a>{' '}
                  <Tag color={PRIORITY_COLORS[p.priority]}>{PRIORITY_LABELS[p.priority]}</Tag>
                  <Progress
                    percent={p.completeness}
                    size="small"
                    format={(v) => `${v}% · ${p.solution_count} GP`}
                  />
                </div>
              ))
            )}
          </Card>
          <Card size="small" title="Hoạt động gần đây">
            {dash.activity.length === 0 ? (
              <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có hoạt động" />
            ) : (
              dash.activity.map((a, i) => (
                <div key={i} style={{ marginBottom: 4 }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>{fmtTime(a.created_at)}</Text>{' '}
                  <Text style={{ fontSize: 13 }}>{a.text}</Text>
                </div>
              ))
            )}
          </Card>
        </Col>
      </Row>
    </div>
  )
}
