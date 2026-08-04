import { useEffect, useMemo, useState } from 'react'
import {
  Button,
  Card,
  Drawer,
  Empty,
  Flex,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Progress,
  Segmented,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import {
  api,
  fmtMoney,
  SOURCE_KIND_LABELS,
  TX_KIND_LABELS,
  type Alloc,
  type ScheduleItem,
  type Source,
  type Tx,
} from './api'

const { Text, Paragraph } = Typography

const kindOptions = Object.entries(SOURCE_KIND_LABELS).map(([value, label]) => ({ value, label }))
const txKindOptions = Object.entries(TX_KIND_LABELS).map(([value, label]) => ({ value, label }))

const KIND_COLORS: Record<string, string> = {
  equity: 'green',
  investor: 'cyan',
  bank_loan: 'volcano',
  credit_line: 'orange',
  personal_loan: 'gold',
  bond: 'purple',
  grant: 'blue',
  other: 'default',
}

const moneyInput = {
  style: { width: '100%' },
  formatter: (v: any) => `${v}`.replace(/\B(?=(\d{3})+(?!\d))/g, ','),
  parser: (v: any) => `${v}`.replace(/,/g, '') as any,
}

// ---------- Nguồn vốn ----------

export function SourcesTab({ onChange }: { onChange: () => void }) {
  const [sources, setSources] = useState<Source[]>([])
  const [editing, setEditing] = useState<Source | 'new' | null>(null)
  const [detailId, setDetailId] = useState<number | null>(null)

  const load = () => api.sources().then((r) => setSources(r.sources)).catch(() => {})
  useEffect(() => {
    load()
  }, [])

  const closeSource = async (id: number) => {
    const r = await api.sourceUpdate(id, { status: 'closed' })
    if (r.error) message.error(r.error)
    else message.success('Đã đóng nguồn vốn')
    load()
    onChange()
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Flex justify="flex-end">
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setEditing('new')}>
          Thêm nguồn vốn
        </Button>
      </Flex>
      <Table
        size="small"
        rowKey="id"
        dataSource={sources}
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        columns={[
          {
            title: 'Nguồn',
            dataIndex: 'name',
            render: (v: string, r: Source) => (
              <Space direction="vertical" size={0}>
                <a onClick={() => setDetailId(r.id)}>{v}</a>
                <Text type="secondary" style={{ fontSize: 12 }}>{r.provider}</Text>
              </Space>
            ),
          },
          {
            title: 'Loại',
            dataIndex: 'kind',
            render: (k: string) => <Tag color={KIND_COLORS[k]}>{SOURCE_KIND_LABELS[k] ?? k}</Tag>,
          },
          {
            title: 'Cam kết / hạn mức',
            dataIndex: 'total_amount',
            align: 'right' as const,
            render: (v: number, r: Source) => fmtMoney(v, r.currency),
          },
          {
            title: 'Dư nợ',
            dataIndex: 'outstanding',
            align: 'right' as const,
            render: (v: number, r: Source) => (
              <Text style={{ color: r.is_debt && v > 0 ? '#f5222d' : undefined }}>{fmtMoney(v, r.currency)}</Text>
            ),
          },
          {
            title: 'Còn rút được',
            dataIndex: 'available',
            align: 'right' as const,
            render: (v: number, r: Source) => fmtMoney(v, r.currency),
          },
          {
            title: 'Lãi suất',
            dataIndex: 'interest_rate',
            align: 'right' as const,
            render: (v: number, r: Source) => (v ? `${v}%/năm${r.rate_type === 'floating' ? ' (thả nổi)' : ''}` : '—'),
          },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            render: (v: string) => <Tag color={v === 'active' ? 'green' : v === 'pending' ? 'orange' : 'default'}>{v}</Tag>,
          },
          {
            title: '',
            key: 'actions',
            render: (_: unknown, r: Source) => (
              <Space>
                <Button size="small" onClick={() => setEditing(r)}>Sửa</Button>
                {r.status === 'active' && (
                  <Popconfirm title="Đóng nguồn vốn này? (chỉ ẩn khỏi chỉ số, không xoá dữ liệu)" onConfirm={() => closeSource(r.id)}>
                    <Button size="small" danger>Đóng</Button>
                  </Popconfirm>
                )}
              </Space>
            ),
          },
        ]}
      />
      <SourceModal
        editing={editing}
        onClose={() => setEditing(null)}
        onSaved={() => {
          setEditing(null)
          load()
          onChange()
        }}
      />
      <SourceDrawer id={detailId} onClose={() => setDetailId(null)} />
    </Space>
  )
}

function SourceModal({
  editing,
  onClose,
  onSaved,
}: {
  editing: Source | 'new' | null
  onClose: () => void
  onSaved: () => void
}) {
  const [form] = Form.useForm()
  const isNew = editing === 'new'

  useEffect(() => {
    if (editing === 'new') form.resetFields()
    else if (editing) form.setFieldsValue(editing)
  }, [editing, form])

  const save = async (v: any) => {
    const body = { ...v, total_amount: Number(v.total_amount || 0), interest_rate: Number(v.interest_rate || 0) }
    const r = isNew ? await api.sourceAdd(body) : await api.sourceUpdate((editing as Source).id, body)
    if (r.error) message.error(r.error)
    else {
      message.success(isNew ? 'Đã thêm nguồn vốn' : 'Đã cập nhật')
      onSaved()
    }
  }

  return (
    <Modal
      title={isNew ? 'Thêm nguồn vốn' : `Sửa nguồn vốn #${typeof editing === 'object' && editing ? editing.id : ''}`}
      open={editing !== null}
      onCancel={onClose}
      onOk={() => form.submit()}
      okText="Lưu"
      cancelText="Huỷ"
      destroyOnHidden
    >
      <Form form={form} layout="vertical" onFinish={save} initialValues={{ kind: 'bank_loan', rate_type: 'fixed', currency: 'VND' }}>
        <Form.Item name="name" label="Tên nguồn" rules={[{ required: true, message: 'Nhập tên nguồn vốn' }]}>
          <Input placeholder="Vay Vietcombank 2026" />
        </Form.Item>
        <Flex gap={12}>
          <Form.Item name="kind" label="Loại" style={{ flex: 1 }} rules={[{ required: true }]}>
            <Select options={kindOptions} />
          </Form.Item>
          <Form.Item name="provider" label="Bên cấp vốn" style={{ flex: 1 }}>
            <Input placeholder="Vietcombank / NĐT A…" />
          </Form.Item>
        </Flex>
        <Flex gap={12}>
          <Form.Item name="total_amount" label="Tổng cam kết / hạn mức" style={{ flex: 2 }}>
            <InputNumber min={0} {...moneyInput} />
          </Form.Item>
          <Form.Item name="currency" label="Tiền tệ" style={{ flex: 1 }}>
            <Input />
          </Form.Item>
        </Flex>
        <Flex gap={12}>
          <Form.Item name="interest_rate" label="Lãi suất (%/năm)" style={{ flex: 1 }}>
            <InputNumber min={0} step={0.1} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="rate_type" label="Kiểu lãi" style={{ flex: 1 }}>
            <Select options={[{ value: 'fixed', label: 'Cố định' }, { value: 'floating', label: 'Thả nổi' }]} />
          </Form.Item>
        </Flex>
        <Flex gap={12}>
          <Form.Item name="start_date" label="Ngày bắt đầu" style={{ flex: 1 }}>
            <Input type="date" />
          </Form.Item>
          <Form.Item name="end_date" label="Ngày đáo hạn" style={{ flex: 1 }}>
            <Input type="date" />
          </Form.Item>
        </Flex>
        <Form.Item name="note" label="Ghi chú">
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

function SourceDrawer({ id, onClose }: { id: number | null; onClose: () => void }) {
  const [detail, setDetail] = useState<{ source: Source; transactions: Tx[]; schedule: ScheduleItem[] } | null>(null)

  useEffect(() => {
    if (id !== null) api.sourceGet(id).then((r) => !r.error && setDetail(r)).catch(() => {})
    else setDetail(null)
  }, [id])

  const s = detail?.source
  return (
    <Drawer title={s ? `${s.name} — ${SOURCE_KIND_LABELS[s.kind] ?? s.kind}` : ''} open={id !== null} onClose={onClose} width={720}>
      {!detail ? null : (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Card size="small">
            <Space size="large" wrap>
              <Text>Cam kết: <b>{fmtMoney(s!.total_amount, s!.currency)}</b></Text>
              <Text>Đã giải ngân: <b>{fmtMoney(s!.disbursed, s!.currency)}</b></Text>
              <Text>Dư nợ: <b style={{ color: '#f5222d' }}>{fmtMoney(s!.outstanding, s!.currency)}</b></Text>
              <Text>Lãi đã trả: <b>{fmtMoney(s!.interest_paid, s!.currency)}</b></Text>
            </Space>
            {s!.note && <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0 }}>{s!.note}</Paragraph>}
          </Card>
          <Card title={`Giao dịch (${detail.transactions.length})`} size="small">
            <TxTable rows={detail.transactions} compact />
          </Card>
          <Card title={`Lịch trả nợ (${detail.schedule.length})`} size="small">
            <ScheduleTable rows={detail.schedule} compact />
          </Card>
        </Space>
      )}
    </Drawer>
  )
}

// ---------- Giao dịch ----------

function TxTable({ rows, compact, onDeleted }: { rows: Tx[]; compact?: boolean; onDeleted?: () => void }) {
  if (!rows.length) return <Empty description="Chưa có giao dịch" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const columns: any[] = [
    { title: 'Ngày', dataIndex: 'tx_date', width: 110 },
    ...(compact ? [] : [{ title: 'Nguồn', dataIndex: 'source_name' }]),
    {
      title: 'Loại',
      dataIndex: 'kind',
      render: (k: string) => (
        <Tag color={k === 'disburse' ? 'green' : k === 'fee' ? 'default' : 'volcano'}>{TX_KIND_LABELS[k] ?? k}</Tag>
      ),
    },
    {
      title: 'Số tiền',
      dataIndex: 'amount',
      align: 'right' as const,
      render: (v: number, r: Tx) => fmtMoney(v, r.currency),
    },
    { title: 'Phân bổ', dataIndex: 'alloc_name', render: (v: string | null) => v ?? '—' },
    { title: 'Ghi chú', dataIndex: 'note', ellipsis: true },
  ]
  if (onDeleted)
    columns.push({
      title: '',
      key: 'del',
      width: 60,
      render: (_: unknown, r: Tx) => (
        <Popconfirm
          title="Xoá giao dịch này khỏi sổ cái?"
          onConfirm={async () => {
            const res = await api.txDelete(r.id)
            if (res.error) message.error(res.error)
            else message.success('Đã xoá')
            onDeleted()
          }}
        >
          <Button size="small" danger type="text">Xoá</Button>
        </Popconfirm>
      ),
    })
  return <Table size="small" rowKey="id" dataSource={rows} pagination={{ pageSize: compact ? 5 : 15, hideOnSinglePage: true }} columns={columns} />
}

export function TxTab({ onChange }: { onChange: () => void }) {
  const [rows, setRows] = useState<Tx[]>([])
  const [sources, setSources] = useState<Source[]>([])
  const [allocs, setAllocs] = useState<Alloc[]>([])
  const [filterSource, setFilterSource] = useState<number | undefined>()
  const [filterKind, setFilterKind] = useState<string | undefined>()
  const [adding, setAdding] = useState(false)
  const [form] = Form.useForm()

  const load = () =>
    api.txList({ source_id: filterSource, kind: filterKind }).then((r) => setRows(r.transactions)).catch(() => {})
  useEffect(() => {
    load()
  }, [filterSource, filterKind])
  useEffect(() => {
    api.sources().then((r) => setSources(r.sources)).catch(() => {})
    api.allocs().then((r) => setAllocs(r.allocations)).catch(() => {})
  }, [])

  const sourceOptions = useMemo(
    () => sources.map((s) => ({ value: s.id, label: `${s.name} (${SOURCE_KIND_LABELS[s.kind] ?? s.kind})` })),
    [sources],
  )

  const add = async (v: any) => {
    const r = await api.txAdd({
      source_id: v.source_id,
      kind: v.kind,
      amount: Number(v.amount),
      tx_date: v.tx_date || undefined,
      alloc_id: v.alloc_id || undefined,
      note: v.note || '',
    })
    if (r.error) message.error(r.error)
    else {
      message.success('Đã ghi giao dịch')
      setAdding(false)
      form.resetFields()
      load()
      onChange()
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Flex justify="space-between" wrap gap={8}>
        <Space wrap>
          <Select
            allowClear
            placeholder="Lọc theo nguồn"
            style={{ minWidth: 220 }}
            options={sourceOptions}
            value={filterSource}
            onChange={setFilterSource}
          />
          <Select
            allowClear
            placeholder="Lọc theo loại"
            style={{ minWidth: 180 }}
            options={txKindOptions}
            value={filterKind}
            onChange={setFilterKind}
          />
        </Space>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
          Ghi giao dịch
        </Button>
      </Flex>
      <TxTable rows={rows} onDeleted={() => { load(); onChange() }} />

      <Modal title="Ghi giao dịch" open={adding} onCancel={() => setAdding(false)} onOk={() => form.submit()} okText="Ghi sổ" cancelText="Huỷ" destroyOnHidden>
        <Form form={form} layout="vertical" onFinish={add} initialValues={{ kind: 'disburse' }}>
          <Form.Item name="source_id" label="Nguồn vốn" rules={[{ required: true, message: 'Chọn nguồn' }]}>
            <Select options={sourceOptions} showSearch optionFilterProp="label" />
          </Form.Item>
          <Flex gap={12}>
            <Form.Item name="kind" label="Loại" style={{ flex: 1 }} rules={[{ required: true }]}>
              <Select options={txKindOptions} />
            </Form.Item>
            <Form.Item name="amount" label="Số tiền" style={{ flex: 1 }} rules={[{ required: true, message: 'Nhập số tiền' }]}>
              <InputNumber min={0.01} {...moneyInput} />
            </Form.Item>
          </Flex>
          <Flex gap={12}>
            <Form.Item name="tx_date" label="Ngày (bỏ trống = hôm nay)" style={{ flex: 1 }}>
              <Input type="date" />
            </Form.Item>
            <Form.Item name="alloc_id" label="Phân bổ vào (tuỳ chọn)" style={{ flex: 1 }}>
              <Select allowClear options={allocs.map((a) => ({ value: a.id, label: a.name }))} />
            </Form.Item>
          </Flex>
          <Form.Item name="note" label="Ghi chú">
            <Input />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  )
}

// ---------- Lịch trả nợ ----------

function ScheduleTable({
  rows,
  compact,
  onPay,
}: {
  rows: ScheduleItem[]
  compact?: boolean
  onPay?: (id: number) => void
}) {
  if (!rows.length) return <Empty description="Chưa có lịch trả nợ — sinh lịch từ một nguồn vay" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const columns: any[] = [
    { title: 'Kỳ', dataIndex: 'seq', width: 50 },
    ...(compact ? [] : [{ title: 'Nguồn', dataIndex: 'source_name' }]),
    { title: 'Đến hạn', dataIndex: 'due_date', width: 110 },
    { title: 'Gốc', dataIndex: 'principal_due', align: 'right' as const, render: (v: number, r: ScheduleItem) => fmtMoney(v, r.currency) },
    { title: 'Lãi', dataIndex: 'interest_due', align: 'right' as const, render: (v: number, r: ScheduleItem) => fmtMoney(v, r.currency) },
    { title: 'Tổng', dataIndex: 'total_due', align: 'right' as const, render: (v: number, r: ScheduleItem) => <b>{fmtMoney(v, r.currency)}</b> },
    {
      title: 'Trạng thái',
      dataIndex: 'status',
      render: (s: string) => <Tag color={s === 'paid' ? 'green' : s === 'overdue' ? 'red' : 'blue'}>{s === 'paid' ? 'đã trả' : s === 'overdue' ? 'quá hạn' : 'sắp tới'}</Tag>,
    },
  ]
  if (onPay)
    columns.push({
      title: '',
      key: 'pay',
      width: 110,
      render: (_: unknown, r: ScheduleItem) =>
        r.status !== 'paid' && (
          <Popconfirm title="Xác nhận bạn ĐÃ thanh toán kỳ này? (app chỉ ghi sổ)" onConfirm={() => onPay(r.id)}>
            <Button size="small" type="primary">Đã trả</Button>
          </Popconfirm>
        ),
    })
  return <Table size="small" rowKey="id" dataSource={rows} pagination={{ pageSize: compact ? 6 : 15, hideOnSinglePage: true }} columns={columns} />
}

export function ScheduleTab({ onChange }: { onChange: () => void }) {
  const [rows, setRows] = useState<ScheduleItem[]>([])
  const [filter, setFilter] = useState<string>('all')
  const [sources, setSources] = useState<Source[]>([])
  const [generating, setGenerating] = useState(false)
  const [form] = Form.useForm()

  const load = () =>
    api
      .schedule(filter === 'all' ? {} : { status: filter })
      .then((r) => setRows(r.schedule))
      .catch(() => {})
  useEffect(() => {
    load()
  }, [filter])
  useEffect(() => {
    api.sources('active').then((r) => setSources(r.sources)).catch(() => {})
  }, [])

  const pay = async (id: number) => {
    const r = await api.schedulePay(id)
    if (r.error) message.error(r.error)
    else message.success('Đã ghi nhận thanh toán (gốc + lãi vào sổ cái)')
    load()
    onChange()
  }

  const generate = async (v: any) => {
    const r = await api.scheduleGenerate({
      source_id: v.source_id,
      method: v.method,
      periods: Number(v.periods),
      principal: v.principal ? Number(v.principal) : undefined,
      annual_rate: v.annual_rate !== undefined && v.annual_rate !== null ? Number(v.annual_rate) : undefined,
      start_date: v.start_date || undefined,
      freq_months: Number(v.freq_months || 1),
    })
    if (r.error) message.error(r.error)
    else {
      message.success('Đã sinh lịch trả nợ')
      setGenerating(false)
      form.resetFields()
      load()
      onChange()
    }
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Flex justify="space-between" wrap gap={8}>
        <Segmented
          value={filter}
          onChange={(v) => setFilter(String(v))}
          options={[
            { label: 'Tất cả', value: 'all' },
            { label: 'Sắp tới', value: 'upcoming' },
            { label: 'Quá hạn', value: 'overdue' },
            { label: 'Đã trả', value: 'paid' },
          ]}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setGenerating(true)}>
          Sinh lịch trả nợ
        </Button>
      </Flex>
      <ScheduleTable rows={rows} onPay={pay} />

      <Modal
        title="Sinh lịch trả nợ"
        open={generating}
        onCancel={() => setGenerating(false)}
        onOk={() => form.submit()}
        okText="Sinh lịch"
        cancelText="Huỷ"
        destroyOnHidden
      >
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Thay thế các kỳ <b>chưa trả</b> của nguồn; kỳ đã trả giữ nguyên. Bỏ trống gốc = dư nợ hiện tại, bỏ trống lãi suất = lãi suất của nguồn.
        </Paragraph>
        <Form form={form} layout="vertical" onFinish={generate} initialValues={{ method: 'annuity', freq_months: 1 }}>
          <Form.Item name="source_id" label="Nguồn vốn" rules={[{ required: true, message: 'Chọn nguồn' }]}>
            <Select
              options={sources.map((s) => ({ value: s.id, label: `${s.name} — dư nợ ${fmtMoney(s.outstanding, s.currency)}` }))}
            />
          </Form.Item>
          <Flex gap={12}>
            <Form.Item name="method" label="Phương pháp" style={{ flex: 2 }}>
              <Select
                options={[
                  { value: 'annuity', label: 'Niên kim (tổng trả mỗi kỳ bằng nhau)' },
                  { value: 'equal_principal', label: 'Gốc chia đều' },
                  { value: 'interest_only', label: 'Trả lãi định kỳ, gốc cuối kỳ' },
                ]}
              />
            </Form.Item>
            <Form.Item name="periods" label="Số kỳ" style={{ flex: 1 }} rules={[{ required: true, message: 'Nhập số kỳ' }]}>
              <InputNumber min={1} max={600} style={{ width: '100%' }} />
            </Form.Item>
          </Flex>
          <Flex gap={12}>
            <Form.Item name="principal" label="Gốc (bỏ trống = dư nợ)" style={{ flex: 2 }}>
              <InputNumber min={0} {...moneyInput} />
            </Form.Item>
            <Form.Item name="annual_rate" label="Lãi %/năm" style={{ flex: 1 }}>
              <InputNumber min={0} step={0.1} style={{ width: '100%' }} />
            </Form.Item>
          </Flex>
          <Flex gap={12}>
            <Form.Item name="start_date" label="Ngày bắt đầu (kỳ 1 = +1 kỳ)" style={{ flex: 1 }}>
              <Input type="date" />
            </Form.Item>
            <Form.Item name="freq_months" label="Chu kỳ" style={{ flex: 1 }}>
              <Select
                options={[
                  { value: 1, label: 'Hằng tháng' },
                  { value: 3, label: 'Hằng quý' },
                  { value: 6, label: '6 tháng' },
                  { value: 12, label: 'Hằng năm' },
                ]}
              />
            </Form.Item>
          </Flex>
        </Form>
      </Modal>
    </Space>
  )
}

// ---------- Phân bổ vốn ----------

export function AllocTab() {
  const [rows, setRows] = useState<Alloc[]>([])
  const [adding, setAdding] = useState(false)
  const [form] = Form.useForm()

  const load = () => api.allocs().then((r) => setRows(r.allocations)).catch(() => {})
  useEffect(() => {
    load()
  }, [])

  const add = async (v: any) => {
    const r = await api.allocAdd({ name: v.name, description: v.description || '', target_amount: Number(v.target_amount || 0) })
    if (r.error) message.error(r.error)
    else {
      message.success('Đã thêm phân bổ')
      setAdding(false)
      form.resetFields()
      load()
    }
  }

  const markDone = async (id: number) => {
    const r = await api.allocUpdate(id, { status: 'done' })
    if (r.error) message.error(r.error)
    load()
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Flex justify="space-between" align="center">
        <Text type="secondary">Gắn giải ngân vào phân bổ (tab Giao dịch) để theo dõi vốn đã rót cho từng mục đích.</Text>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
          Thêm phân bổ
        </Button>
      </Flex>
      <Table
        size="small"
        rowKey="id"
        dataSource={rows}
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        columns={[
          {
            title: 'Mục đích / dự án',
            dataIndex: 'name',
            render: (v: string, r: Alloc) => (
              <Space direction="vertical" size={0}>
                <Text strong>{v}</Text>
                {r.description && <Text type="secondary" style={{ fontSize: 12 }}>{r.description}</Text>}
              </Space>
            ),
          },
          { title: 'Dự kiến', dataIndex: 'target_amount', align: 'right' as const, render: (v: number) => fmtMoney(v) },
          { title: 'Đã rót', dataIndex: 'used', align: 'right' as const, render: (v: number) => fmtMoney(v) },
          {
            title: 'Tiến độ',
            key: 'progress',
            width: 180,
            render: (_: unknown, r: Alloc) => (
              <Progress
                percent={r.target_amount > 0 ? Math.round((r.used / r.target_amount) * 100) : 0}
                size="small"
                status={r.used > r.target_amount && r.target_amount > 0 ? 'exception' : undefined}
              />
            ),
          },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            render: (v: string) => <Tag color={v === 'active' ? 'blue' : 'green'}>{v === 'active' ? 'đang chạy' : 'xong'}</Tag>,
          },
          {
            title: '',
            key: 'actions',
            render: (_: unknown, r: Alloc) =>
              r.status === 'active' && (
                <Button size="small" onClick={() => markDone(r.id)}>Hoàn tất</Button>
              ),
          },
        ]}
      />

      <Modal title="Thêm phân bổ vốn" open={adding} onCancel={() => setAdding(false)} onOk={() => form.submit()} okText="Thêm" cancelText="Huỷ" destroyOnHidden>
        <Form form={form} layout="vertical" onFinish={add}>
          <Form.Item name="name" label="Tên mục đích / dự án" rules={[{ required: true, message: 'Nhập tên' }]}>
            <Input placeholder="Mở xưởng, nhập hàng Q3, marketing…" />
          </Form.Item>
          <Form.Item name="target_amount" label="Ngân sách dự kiến">
            <InputNumber min={0} {...moneyInput} />
          </Form.Item>
          <Form.Item name="description" label="Mô tả">
            <Input.TextArea rows={2} />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  )
}

// ---------- Hoạt động ----------

export function ActivityTab() {
  const [items, setItems] = useState<any[]>([])
  useEffect(() => {
    api.activity().then((a) => setItems(a.activity)).catch(() => {})
  }, [])
  if (!items.length) return <Empty description="Chưa có hoạt động" />
  return (
    <List
      size="small"
      dataSource={items}
      renderItem={(a) => (
        <List.Item>
          <Space>
            <Tag>{a.kind}</Tag>
            <Text>{a.text}</Text>
            {a.ref && <Text type="secondary">(#{a.ref})</Text>}
          </Space>
        </List.Item>
      )}
    />
  )
}
