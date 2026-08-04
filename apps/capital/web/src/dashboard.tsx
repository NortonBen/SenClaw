import { useEffect, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Flex,
  Form,
  Input,
  InputNumber,
  List,
  Progress,
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
import { ExperimentOutlined, SafetyCertificateOutlined, ThunderboltOutlined } from '@ant-design/icons'
import {
  api,
  fmtMoney,
  type CashflowRow,
  type Dashboard,
  type Insight,
  type ScheduleItem,
  type SimResult,
  type SimSide,
  type Source,
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

  const stat = (title: string, value: number, opts: { color?: string; suffix?: string; tip?: string } = {}) => (
    <Col xs={12} md={8} lg={4}>
      <Card size="small">
        <Tooltip title={opts.tip ?? fmtMoney(value)}>
          <Statistic
            title={title}
            value={opts.suffix ? value : fmtCompact(value)}
            suffix={opts.suffix}
            precision={opts.suffix ? 2 : undefined}
            valueStyle={{ color: opts.color, fontSize: 20 }}
          />
        </Tooltip>
      </Card>
    </Col>
  )

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {d.overdue.count > 0 && (
        <Alert
          type="error"
          showIcon
          message={`${d.overdue.count} kỳ trả nợ QUÁ HẠN — tổng ${fmtMoney(d.overdue.total_due)}`}
        />
      )}

      <Row gutter={[12, 12]}>
        {stat('Dư nợ vay', d.debt_outstanding, { color: '#f5222d' })}
        {stat('Vốn chủ đã góp', d.equity_in, { color: '#10b981' })}
        {stat('Còn rút được', d.available)}
        {stat('Lãi đã trả', d.interest_paid)}
        {stat('LS nợ bình quân', d.weighted_debt_rate, { suffix: '%/năm', tip: 'Bình quân gia quyền theo dư nợ' })}
        {d.de_ratio !== null
          ? stat('Hệ số D/E', d.de_ratio, { suffix: '', tip: 'Dư nợ vay / vốn chủ đã góp', color: d.de_ratio > 2 ? '#f5222d' : undefined })
          : stat('Tổng cam kết', d.total_committed)}
      </Row>

      <Row gutter={[12, 12]}>
        <Col xs={24} lg={12}>
          <HealthCard />
        </Col>
        <Col xs={24} lg={12}>
          <SimulateCard sources={d.sources.filter((s) => s.status === 'active')} />
        </Col>
      </Row>

      <Row gutter={[12, 12]}>
        <Col xs={24} lg={12}>
          <Card title={`Sắp đến hạn 30 ngày (${d.upcoming_30d.count}) — ${fmtMoney(d.upcoming_30d.total_due)}`} size="small">
            <DueTable items={d.upcoming_30d.items} />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card title="Dòng tiền 12 tháng" size="small">
            <CashflowChart rows={d.cashflow_12m} />
          </Card>
        </Col>
      </Row>

      <AnalyzeCard />
    </Space>
  )
}

// ---- Đánh giá sức khoẻ vốn (rule engine) ----

const SEVERITY_TAG: Record<string, { color: string; label: string }> = {
  good: { color: 'green', label: 'tốt' },
  warn: { color: 'orange', label: 'cảnh báo' },
  crit: { color: 'red', label: 'nghiêm trọng' },
}

function scoreColor(score: number): string {
  return score >= 85 ? '#10b981' : score >= 70 ? '#1677ff' : score >= 50 ? '#fa8c16' : '#f5222d'
}

function HealthCard() {
  const [ins, setIns] = useState<Insight | null>(null)
  useEffect(() => {
    api.insight().then(setIns).catch(() => {})
  }, [])

  return (
    <Card
      size="small"
      title={
        <Space>
          <SafetyCertificateOutlined style={{ color: '#10b981' }} />
          Đánh giá sức khoẻ vốn
        </Space>
      }
      styles={{ body: { minHeight: 220 } }}
    >
      {!ins ? (
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      ) : (
        <Flex gap={16} align="flex-start">
          <Flex vertical align="center" style={{ minWidth: 132 }}>
            <Progress
              type="dashboard"
              size={120}
              percent={ins.score}
              strokeColor={scoreColor(ins.score)}
              format={(p) => (
                <span>
                  <div style={{ fontSize: 26, fontWeight: 700 }}>{p}</div>
                  <div style={{ fontSize: 12 }}>hạng {ins.grade}</div>
                </span>
              )}
            />
            <Text strong style={{ color: scoreColor(ins.score) }}>{ins.label}</Text>
          </Flex>
          <List
            size="small"
            style={{ flex: 1, maxHeight: 240, overflowY: 'auto' }}
            dataSource={ins.findings}
            renderItem={(f) => (
              <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
                <Space direction="vertical" size={0} style={{ width: '100%' }}>
                  <Space>
                    <Tag color={SEVERITY_TAG[f.severity]?.color}>{SEVERITY_TAG[f.severity]?.label ?? f.severity}</Tag>
                    <Text strong style={{ fontSize: 13 }}>{f.title}</Text>
                  </Space>
                  <Text type="secondary" style={{ fontSize: 12 }}>{f.detail}</Text>
                </Space>
              </List.Item>
            )}
          />
        </Flex>
      )}
    </Card>
  )
}

// ---- Mô phỏng what-if (phân tích hỗ trợ quyết định) ----

function SimulateCard({ sources }: { sources: Source[] }) {
  const [scenario, setScenario] = useState<'new_loan' | 'early_repay'>('new_loan')
  const [result, setResult] = useState<SimResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [form] = Form.useForm()

  const debtSources = sources.filter((s) => s.is_debt && s.outstanding > 0)

  const run = async (v: any) => {
    setLoading(true)
    try {
      const r = await api.simulate(
        scenario === 'new_loan'
          ? {
              scenario,
              amount: Number(v.amount),
              annual_rate: Number(v.annual_rate || 0),
              periods: Number(v.periods || 12),
              method: v.method || 'annuity',
              freq_months: Number(v.freq_months || 1),
            }
          : { scenario, amount: Number(v.amount), source_id: v.source_id },
      )
      setResult(r)
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card
      size="small"
      title={
        <Space>
          <ExperimentOutlined style={{ color: '#10b981' }} />
          Mô phỏng what-if (không ghi sổ)
        </Space>
      }
      styles={{ body: { minHeight: 220 } }}
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <Segmented
          value={scenario}
          onChange={(v) => {
            setScenario(v as any)
            setResult(null)
          }}
          options={[
            { label: 'Vay thêm', value: 'new_loan' },
            { label: 'Trả trước hạn', value: 'early_repay' },
          ]}
        />
        <Form form={form} layout="inline" onFinish={run} style={{ rowGap: 8 }}>
          {scenario === 'new_loan' ? (
            <>
              <Form.Item name="amount" rules={[{ required: true, message: 'Nhập số tiền' }]}>
                <InputNumber
                  min={1}
                  placeholder="Số tiền vay"
                  style={{ width: 150 }}
                  formatter={(v) => `${v}`.replace(/\B(?=(\d{3})+(?!\d))/g, ',')}
                  parser={(v) => `${v}`.replace(/,/g, '') as any}
                />
              </Form.Item>
              <Form.Item name="annual_rate" rules={[{ required: true, message: 'Nhập lãi suất' }]}>
                <InputNumber min={0} step={0.1} placeholder="%/năm" style={{ width: 90 }} />
              </Form.Item>
              <Form.Item name="periods" initialValue={12}>
                <InputNumber min={1} max={600} placeholder="số kỳ" style={{ width: 80 }} />
              </Form.Item>
              <Form.Item name="method" initialValue="annuity">
                <Select
                  style={{ width: 130 }}
                  options={[
                    { value: 'annuity', label: 'Niên kim' },
                    { value: 'equal_principal', label: 'Gốc đều' },
                    { value: 'interest_only', label: 'Lãi định kỳ' },
                  ]}
                />
              </Form.Item>
            </>
          ) : (
            <>
              <Form.Item name="source_id" rules={[{ required: true, message: 'Chọn nguồn' }]}>
                <Select
                  placeholder="Khoản nợ"
                  style={{ minWidth: 210 }}
                  options={debtSources.map((s) => ({
                    value: s.id,
                    label: `${s.name} — dư ${fmtMoney(s.outstanding, s.currency)}`,
                  }))}
                />
              </Form.Item>
              <Form.Item name="amount" rules={[{ required: true, message: 'Nhập số tiền' }]}>
                <InputNumber
                  min={1}
                  placeholder="Số tiền trả trước"
                  style={{ width: 160 }}
                  formatter={(v) => `${v}`.replace(/\B(?=(\d{3})+(?!\d))/g, ',')}
                  parser={(v) => `${v}`.replace(/,/g, '') as any}
                />
              </Form.Item>
            </>
          )}
          <Button type="primary" htmlType="submit" loading={loading}>
            Mô phỏng
          </Button>
        </Form>
        {result?.error && <Alert type="warning" showIcon message={result.error} />}
        {result?.before && result?.after && <SimCompare r={result} />}
      </Space>
    </Card>
  )
}

function SimCompare({ r }: { r: SimResult }) {
  const b = r.before as SimSide
  const a = r.after as SimSide
  const row = (label: string, fb: string, fa: string, worse: boolean) => ({
    key: label,
    label,
    before: fb,
    after: fa,
    worse,
  })
  const rows = [
    row('Dư nợ', fmtMoney(b.debt_outstanding), fmtMoney(a.debt_outstanding), a.debt_outstanding > b.debt_outstanding),
    row('Hệ số D/E', b.de_ratio === null ? '—' : String(b.de_ratio), a.de_ratio === null ? '—' : String(a.de_ratio), (a.de_ratio ?? 0) > (b.de_ratio ?? 0)),
    row('LS bình quân', `${b.weighted_debt_rate}%/năm`, `${a.weighted_debt_rate}%/năm`, a.weighted_debt_rate > b.weighted_debt_rate),
    row('Điểm sức khoẻ', `${b.score} (${b.grade})`, `${a.score} (${a.grade})`, a.score < b.score),
  ]
  return (
    <Space direction="vertical" size="small" style={{ width: '100%' }}>
      {r.loan && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          Kỳ trả đầu: <b>{fmtMoney(r.loan.first_payment)}</b> · Tổng lãi phải trả: <b>{fmtMoney(r.loan.total_interest)}</b> · Tổng chi phí: <b>{fmtMoney(r.loan.total_cost)}</b>
        </Text>
      )}
      {r.estimate && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          Lãi tiết kiệm ước tính: <b style={{ color: '#10b981' }}>{fmtMoney(r.estimate.interest_saved)}</b> — {r.estimate.note}
        </Text>
      )}
      <Table
        size="small"
        pagination={false}
        dataSource={rows}
        columns={[
          { title: '', dataIndex: 'label' },
          { title: 'Trước', dataIndex: 'before', align: 'right' as const },
          {
            title: 'Sau',
            dataIndex: 'after',
            align: 'right' as const,
            render: (v: string, rec: any) => <b style={{ color: rec.worse ? '#f5222d' : '#10b981' }}>{v}</b>,
          },
        ]}
      />
    </Space>
  )
}

function DueTable({ items }: { items: ScheduleItem[] }) {
  if (!items.length) return <Empty description="Không có kỳ nào trong 30 ngày tới" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  return (
    <Table
      size="small"
      rowKey="id"
      pagination={false}
      dataSource={items.slice(0, 8)}
      columns={[
        { title: 'Nguồn', dataIndex: 'source_name' },
        { title: 'Đến hạn', dataIndex: 'due_date', width: 110 },
        {
          title: 'Phải trả',
          dataIndex: 'total_due',
          align: 'right' as const,
          render: (v: number, r: ScheduleItem) => fmtMoney(v, r.currency),
        },
      ]}
    />
  )
}

function CashflowChart({ rows }: { rows: CashflowRow[] }) {
  if (!rows.length) return <Empty description="Chưa có giao dịch" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const max = Math.max(...rows.map((r) => Math.max(r.inflow, r.outflow)), 1)
  return (
    <div>
      <Flex gap={6} align="end" style={{ height: 140, padding: '0 4px' }}>
        {rows.map((r) => (
          <Tooltip
            key={r.month}
            title={
              <>
                <div>{r.month}</div>
                <div>Vào: {fmtMoney(r.inflow)}</div>
                <div>Ra: {fmtMoney(r.outflow)}</div>
                <div>Ròng: {fmtMoney(r.net)}</div>
              </>
            }
          >
            <Flex gap={2} align="end" style={{ flex: 1, height: '100%', cursor: 'default' }}>
              <div style={{ flex: 1, height: `${(r.inflow / max) * 100}%`, background: '#10b981', borderRadius: 3, minHeight: 2 }} />
              <div style={{ flex: 1, height: `${(r.outflow / max) * 100}%`, background: '#f5222d', borderRadius: 3, minHeight: 2 }} />
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
          Tiền vào (giải ngân)
        </Text>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#f5222d', borderRadius: 2, marginRight: 4 }} />
          Tiền ra (gốc + lãi + phí)
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
          <ThunderboltOutlined style={{ color: '#10b981' }} />
          AI phân tích nguồn vốn
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
