import { useEffect, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Flex,
  Input,
  Row,
  Spin,
  Statistic,
  Table,
  Tag,
  Typography,
} from 'antd'
import { ReloadOutlined, RobotOutlined } from '@ant-design/icons'
import { api, fmtDate, fmtMoney, fmtQty, type Dashboard } from './api'
import { BarChart } from './chart'

const { Paragraph, Text } = Typography

export function DashboardTab() {
  const [data, setData] = useState<Dashboard | null>(null)
  const [loading, setLoading] = useState(true)

  const load = () => {
    setLoading(true)
    api
      .dashboard()
      .then(setData)
      .finally(() => setLoading(false))
  }
  useEffect(load, [])

  if (loading && !data)
    return (
      <Flex justify="center" style={{ padding: 48 }}>
        <Spin />
      </Flex>
    )
  if (!data) return <Empty description="Không tải được dữ liệu" />

  return (
    <Flex vertical gap={12}>
      <Row gutter={[12, 12]}>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic title="Doanh thu hôm nay" value={data.today.revenue} formatter={(v) => fmtMoney(Number(v))} />
          </Card>
        </Col>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic title="Đơn hôm nay" value={data.today.orders} />
          </Card>
        </Col>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic
              title="Lãi gộp hôm nay"
              value={data.today.profit}
              formatter={(v) => fmtMoney(Number(v))}
              valueStyle={{ color: data.today.profit >= 0 ? '#10b981' : '#f5222d' }}
            />
          </Card>
        </Col>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic title="Doanh thu 7 ngày" value={data.last7.revenue} formatter={(v) => fmtMoney(Number(v))} />
          </Card>
        </Col>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic title="Lãi gộp 7 ngày" value={data.last7.profit} formatter={(v) => fmtMoney(Number(v))} />
          </Card>
        </Col>
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic title="Giá trị tồn kho" value={data.stock_value} formatter={(v) => fmtMoney(Number(v))} />
          </Card>
        </Col>
      </Row>

      {data.alerts.length > 0 && (
        <Flex vertical gap={6}>
          {data.alerts.map((a, i) => (
            <Alert key={i} type={a.includes('ÂM') ? 'error' : 'warning'} showIcon message={a} />
          ))}
        </Flex>
      )}

      <Card
        size="small"
        title="Doanh thu 14 ngày"
        extra={<Button size="small" icon={<ReloadOutlined />} onClick={load} />}
      >
        <BarChart
          points={data.revenue_14d.map((p) => ({
            label: p.date.slice(8), // dd
            title: fmtDate(p.date),
            a: p.revenue,
            b: p.profit,
          }))}
          aName="Doanh thu"
          bName="Lãi gộp"
          empty="Chưa có đơn bán nào"
        />
      </Card>

      <Row gutter={[12, 12]}>
        <Col xs={24} lg={12}>
          <Card size="small" title="Top món 7 ngày">
            <Table
              size="small"
              rowKey="name"
              dataSource={data.top_items_7d}
              pagination={false}
              locale={{ emptyText: 'Chưa có dữ liệu bán' }}
              columns={[
                { title: 'Món', dataIndex: 'name' },
                { title: 'SL', dataIndex: 'qty', align: 'right', render: fmtQty },
                { title: 'Doanh thu', dataIndex: 'revenue', align: 'right', render: fmtMoney },
              ]}
            />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card size="small" title="Đơn gần đây">
            <Table
              size="small"
              rowKey="id"
              dataSource={data.recent_sales}
              pagination={false}
              locale={{ emptyText: 'Chưa có đơn' }}
              columns={[
                { title: 'Mã', dataIndex: 'code', render: (c, r) => <Text delete={r.status === 'void'}>{c}</Text> },
                { title: 'Ngày', dataIndex: 'sale_date', render: fmtDate },
                { title: 'Món', dataIndex: 'items', ellipsis: true },
                { title: 'Tổng', dataIndex: 'total', align: 'right', render: fmtMoney },
              ]}
            />
          </Card>
        </Col>
      </Row>

      <AnalyzeCard />
    </Flex>
  )
}

function AnalyzeCard() {
  const [question, setQuestion] = useState('')
  const [loading, setLoading] = useState(false)
  const [result, setResult] = useState<{ analysis: string; model: string } | null>(null)

  const run = async () => {
    setLoading(true)
    try {
      setResult(await api.analyze(question))
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card size="small" title={<><RobotOutlined /> AI phân tích kinh doanh</>}>
      <Flex gap={8}>
        <Input
          placeholder="Câu hỏi (bỏ trống = phân tích tổng quan)…"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onPressEnter={run}
        />
        <Button type="primary" loading={loading} onClick={run}>
          Phân tích
        </Button>
      </Flex>
      {result && (
        <div style={{ marginTop: 12 }}>
          <Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 4 }}>{result.analysis}</Paragraph>
          {result.model && <Tag>{result.model}</Tag>}
        </div>
      )}
    </Card>
  )
}
