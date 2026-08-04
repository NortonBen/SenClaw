import { useEffect, useState } from 'react'
import { Alert, Card, Col, Flex, Row, Segmented, Statistic, Table, Tag, Typography } from 'antd'
import {
  api,
  fmtDate,
  fmtMoney,
  fmtQty,
  type ForecastIngredients,
  type ForecastSales,
  type PurchaseSuggest,
} from './api'
import { BarChart } from './chart'

const { Text } = Typography

export function ForecastTab() {
  const [days, setDays] = useState(7)
  const [sales, setSales] = useState<ForecastSales | null>(null)
  const [ings, setIngs] = useState<ForecastIngredients | null>(null)
  const [suggest, setSuggest] = useState<PurchaseSuggest | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    Promise.all([api.forecastSales(days), api.forecastIngredients(days), api.purchaseSuggest(days)])
      .then(([s, i, g]) => {
        setSales(s)
        setIngs(i)
        setSuggest(g)
      })
      .finally(() => setLoading(false))
  }, [days])

  const daysLeftTag = (v: number | null) => {
    if (v === null || v === undefined) return <Text type="secondary">—</Text>
    if (v <= 2) return <Tag color="red">{v} ngày</Tag>
    if (v <= 7) return <Tag color="orange">{v} ngày</Tag>
    return <Tag color="green">{v} ngày</Tag>
  }

  return (
    <Flex vertical gap={12}>
      <Flex align="center" gap={12}>
        <Text>Dự đoán cho</Text>
        <Segmented
          value={days}
          onChange={(v) => setDays(Number(v))}
          options={[
            { value: 7, label: '7 ngày' },
            { value: 14, label: '14 ngày' },
            { value: 30, label: '30 ngày' },
          ]}
        />
      </Flex>

      <Card size="small" title={`Dự đoán doanh thu ${days} ngày tới`} loading={loading && !sales}>
        {sales && (
          <Flex vertical gap={12}>
            <Row gutter={[12, 12]}>
              <Col xs={12} md={6}>
                <Statistic title="Doanh thu dự kiến" value={sales.total_revenue} formatter={(v) => fmtMoney(Number(v))} />
              </Col>
              <Col xs={12} md={6}>
                <Statistic title="Lãi gộp dự kiến" value={sales.total_profit} formatter={(v) => fmtMoney(Number(v))} />
              </Col>
            </Row>
            <BarChart
              points={sales.future.map((p) => ({ label: p.date.slice(8), title: fmtDate(p.date), a: p.revenue, b: p.profit }))}
              aName="Doanh thu dự kiến"
              bName="Lãi gộp dự kiến"
              empty="Chưa đủ lịch sử bán để dự đoán"
            />
            <Table
              size="small"
              rowKey="menu_id"
              dataSource={sales.items}
              pagination={{ pageSize: 10, hideOnSinglePage: true }}
              locale={{ emptyText: 'Chưa đủ lịch sử bán (cần đơn trong 28 ngày gần nhất)' }}
              columns={[
                { title: 'Món', dataIndex: 'name' },
                { title: 'SL dự kiến', dataIndex: 'forecast_qty', align: 'right', render: fmtQty },
                { title: 'Doanh thu dự kiến', dataIndex: 'forecast_revenue', align: 'right', render: fmtMoney },
                { title: 'Lãi gộp dự kiến', dataIndex: 'forecast_profit', align: 'right', render: fmtMoney },
              ]}
            />
            <Alert type="info" showIcon message={sales.note} />
          </Flex>
        )}
      </Card>

      <Card size="small" title={`Dự báo nguyên liệu ${days} ngày tới`} loading={loading && !ings}>
        {ings && (
          <Table
            size="small"
            rowKey="ingredient_id"
            dataSource={ings.rows}
            pagination={{ pageSize: 12, hideOnSinglePage: true }}
            locale={{ emptyText: 'Chưa có dữ liệu tiêu hao' }}
            columns={[
              { title: 'Nguyên liệu', dataIndex: 'name' },
              { title: 'Tồn hiện tại', dataIndex: 'stock_display', align: 'right' },
              { title: 'Tiêu hao dự kiến', dataIndex: 'usage_display', align: 'right' },
              { title: 'Còn đủ', dataIndex: 'days_left', align: 'right', render: daysLeftTag },
              { title: 'Dự kiến hết', dataIndex: 'stockout_date', render: (v) => (v ? fmtDate(v) : '—') },
              {
                title: 'Cần nhập',
                dataIndex: 'need_display',
                align: 'right',
                render: (v, r) => (r.need > 0 ? <Text strong>{v}</Text> : '—'),
              },
            ]}
          />
        )}
      </Card>

      <Card size="small" title={`Đề xuất nhập hàng cho ${days} ngày tới`} loading={loading && !suggest}>
        {suggest && (
          <Flex vertical gap={8}>
            <Table
              size="small"
              rowKey="ingredient_id"
              dataSource={suggest.rows}
              pagination={false}
              locale={{ emptyText: 'Kho đang đủ dùng — chưa cần nhập gì 🎉' }}
              columns={[
                { title: 'Nguyên liệu', dataIndex: 'name' },
                { title: 'Tồn', dataIndex: 'stock_display', align: 'right' },
                { title: 'Cần nhập', dataIndex: 'need_display', align: 'right', render: (v) => <Text strong>{v}</Text> },
                { title: 'Chi phí ước tính', dataIndex: 'est_cost', align: 'right', render: fmtMoney },
              ]}
            />
            {suggest.rows.length > 0 && (
              <Flex justify="end">
                <Text strong>Tổng chi phí ước tính: {fmtMoney(suggest.est_total_cost)}</Text>
              </Flex>
            )}
            <Alert type="info" showIcon message={suggest.note} />
          </Flex>
        )}
      </Card>
    </Flex>
  )
}
