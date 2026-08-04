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
  Space,
  Spin,
  Statistic,
  Table,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import { ThunderboltOutlined } from '@ant-design/icons'
import {
  api,
  fmtMoney,
  fmtQty,
  MOVE_KIND_COLORS,
  MOVE_KIND_LABELS,
  type Dashboard,
  type InoutRow,
  type Move,
  type Product,
} from './api'

const { Paragraph, Text } = Typography

function fmtCompact(v: number): string {
  return new Intl.NumberFormat('vi-VN', { notation: 'compact', maximumFractionDigits: 1 }).format(v)
}

export default function DashboardTab() {
  const [d, setD] = useState<Dashboard | null>(null)

  useEffect(() => {
    api.dashboard().then(setD).catch(() => {})
  }, [])

  if (!d) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  const stat = (title: string, value: number, opts: { color?: string; money?: boolean; tip?: string } = {}) => (
    <Col xs={12} md={8} lg={4}>
      <Card size="small">
        <Tooltip title={opts.tip ?? (opts.money ? fmtMoney(value) : fmtQty(value))}>
          <Statistic
            title={title}
            value={opts.money ? fmtCompact(value) : value}
            valueStyle={{ color: opts.color, fontSize: 20 }}
          />
        </Tooltip>
      </Card>
    </Col>
  )

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {d.low_stock.count > 0 && (
        <Alert
          type="warning"
          showIcon
          message={`${d.low_stock.count} mặt hàng dưới tồn tối thiểu — cần nhập thêm`}
        />
      )}

      <Row gutter={[12, 12]}>
        {stat('Giá trị tồn kho', d.stock_value, { money: true, color: '#f59e0b' })}
        {stat('Sản phẩm đang bán', d.products_active)}
        {stat('Sắp hết hàng', d.low_stock.count, { color: d.low_stock.count > 0 ? '#fa8c16' : undefined })}
        {stat('Hết hàng', d.out_of_stock_count, { color: d.out_of_stock_count > 0 ? '#f5222d' : undefined })}
        {stat('Nhập 30 ngày', d.in_30d.value, { money: true, color: '#10b981', tip: `${fmtQty(d.in_30d.qty)} đơn vị — ${fmtMoney(d.in_30d.value)}` })}
        {stat('Xuất 30 ngày', d.out_30d.value, { money: true, color: '#f5222d', tip: `${fmtQty(d.out_30d.qty)} đơn vị — ${fmtMoney(d.out_30d.value)}` })}
      </Row>

      <Row gutter={[12, 12]}>
        <Col xs={24} lg={12}>
          <Card title={`Hàng dưới tồn tối thiểu (${d.low_stock.count})`} size="small">
            <LowStockTable items={d.low_stock.items} />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card title="Nhập – xuất 12 tháng (giá trị)" size="small">
            <InoutChart rows={d.inout_12m} />
          </Card>
        </Col>
      </Row>

      <Row gutter={[12, 12]}>
        <Col xs={24} lg={12}>
          <Card title="Top sản phẩm theo giá trị tồn" size="small">
            <TopProducts items={d.top_products} />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card title="Phiếu kho gần đây" size="small">
            <RecentMoves items={d.recent_moves} />
          </Card>
        </Col>
      </Row>

      <AnalyzeCard />
    </Space>
  )
}

function LowStockTable({ items }: { items: Product[] }) {
  if (!items.length) return <Empty description="Không có mặt hàng nào dưới tồn tối thiểu" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  return (
    <Table
      size="small"
      rowKey="id"
      pagination={false}
      dataSource={items.slice(0, 8)}
      columns={[
        { title: 'Sản phẩm', dataIndex: 'name', render: (v: string, r: Product) => `${v}${r.sku ? ` (${r.sku})` : ''}` },
        {
          title: 'Tồn',
          dataIndex: 'on_hand',
          align: 'right' as const,
          render: (v: number, r: Product) => (
            <Text style={{ color: '#fa8c16' }}>
              {fmtQty(v)} / {fmtQty(r.min_stock)} {r.unit}
            </Text>
          ),
        },
      ]}
    />
  )
}

function TopProducts({ items }: { items: Product[] }) {
  if (!items.length) return <Empty description="Chưa có sản phẩm" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  return (
    <Table
      size="small"
      rowKey="id"
      pagination={false}
      dataSource={items}
      columns={[
        { title: 'Sản phẩm', dataIndex: 'name' },
        { title: 'Tồn', dataIndex: 'on_hand', align: 'right' as const, render: (v: number, r: Product) => `${fmtQty(v)} ${r.unit}` },
        { title: 'Giá trị', dataIndex: 'stock_value', align: 'right' as const, render: (v: number) => fmtMoney(v) },
      ]}
    />
  )
}

function RecentMoves({ items }: { items: Move[] }) {
  if (!items.length) return <Empty description="Chưa có phiếu kho" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  return (
    <Table
      size="small"
      rowKey="id"
      pagination={false}
      dataSource={items.slice(0, 8)}
      columns={[
        { title: 'Mã', dataIndex: 'code', width: 90 },
        {
          title: 'Loại',
          dataIndex: 'kind',
          width: 110,
          render: (k: string) => <Tag color={MOVE_KIND_COLORS[k]}>{MOVE_KIND_LABELS[k] ?? k}</Tag>,
        },
        { title: 'Ngày', dataIndex: 'move_date', width: 105 },
        { title: 'Kho', dataIndex: 'warehouse_name', ellipsis: true },
        { title: 'Giá trị', dataIndex: 'total_value', align: 'right' as const, render: (v: number) => fmtMoney(v) },
      ]}
    />
  )
}

function InoutChart({ rows }: { rows: InoutRow[] }) {
  if (!rows.length) return <Empty description="Chưa có phiếu kho" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const max = Math.max(...rows.map((r) => Math.max(r.in_value, r.out_value)), 1)
  return (
    <div>
      <Flex gap={6} align="end" style={{ height: 140, padding: '0 4px' }}>
        {rows.map((r) => (
          <Tooltip
            key={r.month}
            title={
              <>
                <div>{r.month}</div>
                <div>Nhập: {fmtMoney(r.in_value)} ({fmtQty(r.in_qty)})</div>
                <div>Xuất: {fmtMoney(r.out_value)} ({fmtQty(r.out_qty)})</div>
                {r.adjust_qty !== 0 && <div>Điều chỉnh: {fmtQty(r.adjust_qty)}</div>}
              </>
            }
          >
            <Flex gap={2} align="end" style={{ flex: 1, height: '100%', cursor: 'default' }}>
              <div style={{ flex: 1, height: `${(r.in_value / max) * 100}%`, background: '#10b981', borderRadius: 3, minHeight: 2 }} />
              <div style={{ flex: 1, height: `${(r.out_value / max) * 100}%`, background: '#f5222d', borderRadius: 3, minHeight: 2 }} />
            </Flex>
          </Tooltip>
        ))}
      </Flex>
      <Flex gap={6} style={{ padding: '4px 4px 0' }}>
        {rows.map((r) => (
          <Text key={r.month} type="secondary" style={{ flex: 1, fontSize: 10, textAlign: 'center' }}>
            {r.month.slice(5)}
          </Text>
        ))}
      </Flex>
      <Space size="large" style={{ marginTop: 8 }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#10b981', borderRadius: 2, marginRight: 4 }} />
          Nhập kho
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#f5222d', borderRadius: 2, marginRight: 4 }} />
          Xuất kho
        </Text>
      </Space>
    </div>
  )
}

function AnalyzeCard() {
  const [question, setQuestion] = useState('')
  const [result, setResult] = useState<{ analysis: string; model: string } | null>(null)
  const [loading, setLoading] = useState(false)

  const run = async () => {
    setLoading(true)
    try {
      setResult(await api.analyze(question))
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
          AI phân tích tồn kho
        </Space>
      }
    >
      <Flex gap={8}>
        <Input
          placeholder="Câu hỏi cụ thể (bỏ trống = phân tích tổng quan)…"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onPressEnter={run}
        />
        <Button type="primary" loading={loading} onClick={run}>
          Phân tích
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
