import { useEffect, useState } from 'react'
import { Button, Card, Col, Flex, Input, Row, Segmented, Statistic, Table } from 'antd'
import { addDaysISO, api, fmtDate, fmtMoney, fmtQty, todayISO, type RevenueReport } from './api'
import { BarChart } from './chart'

export function ReportsTab() {
  const [from, setFrom] = useState(addDaysISO(todayISO(), -29))
  const [to, setTo] = useState(todayISO())
  const [groupBy, setGroupBy] = useState('day')
  const [report, setReport] = useState<RevenueReport | null>(null)
  const [loading, setLoading] = useState(false)

  const load = (f = from, t = to, g = groupBy) => {
    setLoading(true)
    api
      .revenueReport({ from: f, to: t, group_by: g })
      .then(setReport)
      .finally(() => setLoading(false))
  }
  useEffect(() => {
    load(from, to, groupBy)
  }, [groupBy])

  const preset = (days: number) => {
    const f = addDaysISO(todayISO(), -(days - 1))
    const t = todayISO()
    setFrom(f)
    setTo(t)
    load(f, t, groupBy)
  }

  const columns =
    groupBy === 'item'
      ? [
          { title: 'Món', dataIndex: 'item' },
          { title: 'SL', dataIndex: 'qty', align: 'right' as const, render: fmtQty },
          { title: 'Doanh thu', dataIndex: 'revenue', align: 'right' as const, render: fmtMoney },
          { title: 'Giá vốn', dataIndex: 'cogs', align: 'right' as const, render: fmtMoney },
          { title: 'Lãi gộp', dataIndex: 'profit', align: 'right' as const, render: fmtMoney },
          { title: 'Margin %', dataIndex: 'margin_pct', align: 'right' as const },
        ]
      : groupBy === 'category'
        ? [
            { title: 'Nhóm', dataIndex: 'category' },
            { title: 'SL', dataIndex: 'qty', align: 'right' as const, render: fmtQty },
            { title: 'Doanh thu', dataIndex: 'revenue', align: 'right' as const, render: fmtMoney },
            { title: 'Giá vốn', dataIndex: 'cogs', align: 'right' as const, render: fmtMoney },
            { title: 'Lãi gộp', dataIndex: 'profit', align: 'right' as const, render: fmtMoney },
            { title: 'Margin %', dataIndex: 'margin_pct', align: 'right' as const },
          ]
        : [
            { title: 'Ngày', dataIndex: 'date', render: fmtDate },
            { title: 'Số đơn', dataIndex: 'orders', align: 'right' as const },
            { title: 'Doanh thu', dataIndex: 'revenue', align: 'right' as const, render: fmtMoney },
            { title: 'Giá vốn', dataIndex: 'cogs', align: 'right' as const, render: fmtMoney },
            { title: 'Lãi gộp', dataIndex: 'profit', align: 'right' as const, render: fmtMoney },
          ]

  return (
    <Flex vertical gap={12}>
      <Card
        size="small"
        title="Báo cáo doanh thu"
        extra={
          <Segmented
            size="small"
            value={groupBy}
            onChange={(v) => setGroupBy(String(v))}
            options={[
              { value: 'day', label: 'Theo ngày' },
              { value: 'item', label: 'Theo món' },
              { value: 'category', label: 'Theo nhóm' },
            ]}
          />
        }
      >
        <Flex gap={8} wrap style={{ marginBottom: 12 }}>
          <Input style={{ width: 130 }} value={from} onChange={(e) => setFrom(e.target.value)} placeholder="Từ YYYY-MM-DD" />
          <Input style={{ width: 130 }} value={to} onChange={(e) => setTo(e.target.value)} placeholder="Đến YYYY-MM-DD" />
          <Button onClick={() => load()} loading={loading}>
            Xem
          </Button>
          <Button size="small" onClick={() => preset(7)}>
            7 ngày
          </Button>
          <Button size="small" onClick={() => preset(30)}>
            30 ngày
          </Button>
        </Flex>
        {report && (
          <Row gutter={[12, 12]} style={{ marginBottom: 12 }}>
            <Col xs={12} md={4}>
              <Statistic title="Số đơn" value={report.orders} />
            </Col>
            <Col xs={12} md={4}>
              <Statistic title="Ly/phần bán" value={report.items_sold} formatter={(v) => fmtQty(Number(v))} />
            </Col>
            <Col xs={12} md={5}>
              <Statistic title="Doanh thu" value={report.revenue} formatter={(v) => fmtMoney(Number(v))} />
            </Col>
            <Col xs={12} md={5}>
              <Statistic title="Giá vốn" value={report.cogs} formatter={(v) => fmtMoney(Number(v))} />
            </Col>
            <Col xs={12} md={6}>
              <Statistic
                title="Lãi gộp"
                value={report.profit}
                formatter={(v) => fmtMoney(Number(v))}
                valueStyle={{ color: report.profit >= 0 ? '#10b981' : '#f5222d' }}
              />
            </Col>
          </Row>
        )}
        {report && groupBy === 'day' && report.rows.length > 0 && (
          <div style={{ marginBottom: 12 }}>
            <BarChart
              points={report.rows.map((r: any) => ({ label: String(r.date).slice(8), title: fmtDate(r.date), a: r.revenue, b: r.profit }))}
              aName="Doanh thu"
              bName="Lãi gộp"
            />
          </div>
        )}
        <Table
          size="small"
          rowKey={(_, i) => String(i)}
          loading={loading}
          dataSource={report?.rows ?? []}
          pagination={{ pageSize: 15, hideOnSinglePage: true }}
          columns={columns}
          locale={{ emptyText: 'Không có đơn trong khoảng này' }}
        />
      </Card>
    </Flex>
  )
}
