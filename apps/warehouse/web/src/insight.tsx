import { useEffect, useState } from 'react'
import {
  Button,
  Card,
  Col,
  Flex,
  Input,
  Row,
  Segmented,
  Select,
  Space,
  Spin,
  Statistic,
  Table,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import { RiseOutlined, ThunderboltOutlined } from '@ant-design/icons'
import { api, CLASS_COLORS, CLASS_LABELS, fmtMoney, fmtQty, type Insight, type InsightItem } from './api'

const { Paragraph, Text } = Typography

function fmtCompact(v: number): string {
  return new Intl.NumberFormat('vi-VN', { notation: 'compact', maximumFractionDigits: 1 }).format(v)
}

/// Tab "Phân tích SP": phân loại tiềm năng / ổn định / bán chậm / tồn đọng
/// (rule-based từ backend) + AI nhận định danh mục qua bridge.
export default function InsightTab() {
  const [days, setDays] = useState(90)
  const [data, setData] = useState<Insight | null>(null)
  const [filter, setFilter] = useState<string>('all')

  useEffect(() => {
    setData(null)
    api.insight(days).then(setData).catch(() => {})
  }, [days])

  if (!data) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  const s = data.summary
  const items = filter === 'all' ? data.items : data.items.filter((i) => i.class === filter)

  const stat = (title: string, value: number | string, opts: { color?: string; tip?: string } = {}) => (
    <Col xs={12} md={8} lg={4}>
      <Card size="small">
        <Tooltip title={opts.tip}>
          <Statistic title={title} value={value} valueStyle={{ color: opts.color, fontSize: 20 }} />
        </Tooltip>
      </Card>
    </Col>
  )

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Row gutter={[12, 12]}>
        {stat('Tiềm năng', s.potential_count, { color: '#10b981', tip: 'Đang bán tốt, tồn chỉ đủ ≤45 ngày — nên nhập thêm' })}
        {stat('Ổn định', s.steady_count)}
        {stat('Bán chậm', s.slow_count, { color: '#fa8c16', tip: 'Tồn đủ bán trên 180 ngày' })}
        {stat('Tồn đọng', s.dead_count, { color: '#f5222d', tip: `Không bán được đơn nào trong ${data.window_days} ngày` })}
        {stat('Vốn chôn tồn đọng', fmtCompact(s.dead_stock_value), { color: '#f5222d', tip: fmtMoney(s.dead_stock_value) })}
        <Col xs={12} md={8} lg={4}>
          <Card size="small">
            <Statistic
              title="Cửa sổ phân tích"
              valueRender={() => (
                <Select
                  size="small"
                  value={days}
                  onChange={setDays}
                  options={[30, 60, 90, 180, 365].map((d) => ({ value: d, label: `${d} ngày` }))}
                />
              )}
            />
          </Card>
        </Col>
      </Row>

      <Card
        size="small"
        title={
          <Space>
            <RiseOutlined style={{ color: '#f59e0b' }} />
            Hiệu suất sản phẩm ({data.window_days} ngày)
          </Space>
        }
        extra={
          <Segmented
            size="small"
            value={filter}
            onChange={(v) => setFilter(v as string)}
            options={[
              { label: `Tất cả (${data.items.length})`, value: 'all' },
              { label: `Tiềm năng (${s.potential_count})`, value: 'potential' },
              { label: `Bán chậm (${s.slow_count})`, value: 'slow' },
              { label: `Tồn đọng (${s.dead_count})`, value: 'dead' },
            ]}
          />
        }
      >
        <Table
          size="small"
          rowKey="id"
          dataSource={items}
          pagination={{ pageSize: 12, hideOnSinglePage: true }}
          columns={[
            {
              title: 'Sản phẩm',
              dataIndex: 'name',
              render: (v: string, r: InsightItem) => `${v}${r.sku ? ` (${r.sku})` : ''}`,
            },
            {
              title: 'Phân loại',
              dataIndex: 'class',
              width: 120,
              render: (c: string) => <Tag color={CLASS_COLORS[c]}>{CLASS_LABELS[c] ?? c}</Tag>,
            },
            {
              title: `Bán ${data.window_days}ng`,
              dataIndex: 'sold_qty',
              align: 'right' as const,
              width: 100,
              render: (v: number, r: InsightItem) => (v ? `${fmtQty(v)} ${r.unit}` : '—'),
            },
            {
              title: 'Doanh số',
              dataIndex: 'sold_value',
              align: 'right' as const,
              width: 120,
              render: (v: number) => (v ? fmtMoney(v) : '—'),
            },
            {
              title: 'Tốc độ /30ng',
              dataIndex: 'velocity_30d',
              align: 'right' as const,
              width: 105,
              render: (v: number) => (v ? fmtQty(v) : '—'),
            },
            {
              title: 'Tồn còn đủ',
              dataIndex: 'days_of_stock',
              align: 'right' as const,
              width: 100,
              render: (v: number | null) =>
                v === null ? (
                  '—'
                ) : (
                  <Text style={{ color: v <= 45 ? '#10b981' : v > 180 ? '#fa8c16' : undefined }}>{v} ngày</Text>
                ),
            },
            {
              title: 'Biên lãi',
              dataIndex: 'margin_pct',
              align: 'right' as const,
              width: 90,
              render: (v: number | null) => (v === null ? '—' : `${v}%`),
            },
            {
              title: 'Tồn / Giá trị',
              dataIndex: 'stock_value',
              align: 'right' as const,
              width: 140,
              render: (v: number, r: InsightItem) => `${fmtQty(r.on_hand)} · ${fmtMoney(v)}`,
            },
            {
              title: 'Bán lần cuối',
              dataIndex: 'last_sale_date',
              width: 110,
              render: (v: string | null) => v ?? '—',
            },
          ]}
        />
      </Card>

      <AnalyzeProductsCard days={days} />
    </Space>
  )
}

function AnalyzeProductsCard({ days }: { days: number }) {
  const [question, setQuestion] = useState('')
  const [result, setResult] = useState<{ analysis: string; model: string } | null>(null)
  const [loading, setLoading] = useState(false)

  const run = async () => {
    setLoading(true)
    try {
      setResult(await api.analyzeProducts(question, days))
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card
      size="small"
      title={
        <Space>
          <ThunderboltOutlined style={{ color: '#f59e0b' }} />
          AI đánh giá danh mục — tiềm năng &amp; tồn đọng
        </Space>
      }
    >
      <Flex gap={8}>
        <Input
          placeholder="Câu hỏi cụ thể (bỏ trống = sản phẩm nào nên nhập thêm, hàng nào cần xả)…"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onPressEnter={run}
        />
        <Button type="primary" loading={loading} onClick={run}>
          Đánh giá
        </Button>
      </Flex>
      {result && (
        <>
          <Paragraph style={{ whiteSpace: 'pre-wrap', marginTop: 16, marginBottom: 4 }}>{result.analysis}</Paragraph>
          {result.model && <Tag>{result.model}</Tag>}
        </>
      )}
    </Card>
  )
}
