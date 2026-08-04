import { useEffect, useState } from 'react'
import {
  Button,
  Card,
  Empty,
  Flex,
  Input,
  InputNumber,
  Modal,
  Segmented,
  Select,
  Table,
  Typography,
  message,
} from 'antd'
import { DeleteOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  api,
  fmtDate,
  fmtMoney,
  fmtQty,
  PURCHASE_UNITS,
  todayISO,
  type Ingredient,
  type Purchase,
  type PurchaseReport,
} from './api'

const { Text } = Typography

interface DraftLine {
  key: number
  ingredient_id: number | null
  qty: number | null
  unit: string
  unit_price: number | null
}

export function PurchasesTab({ onChange }: { onChange: () => void }) {
  const [ingredients, setIngredients] = useState<Ingredient[]>([])
  const [reloadKey, setReloadKey] = useState(0)

  useEffect(() => {
    api.ingredients().then((r) => setIngredients(r.ingredients))
  }, [reloadKey])

  return (
    <Flex vertical gap={12}>
      <CreatePurchase
        ingredients={ingredients}
        onCreated={() => {
          setReloadKey((k) => k + 1)
          onChange()
        }}
      />
      <PurchaseList reloadKey={reloadKey} />
      <PurchaseReportCard />
    </Flex>
  )
}

function CreatePurchase({ ingredients, onCreated }: { ingredients: Ingredient[]; onCreated: () => void }) {
  const [supplier, setSupplier] = useState('')
  const [date, setDate] = useState(todayISO())
  const [note, setNote] = useState('')
  const [lines, setLines] = useState<DraftLine[]>([{ key: 1, ingredient_id: null, qty: null, unit: 'g', unit_price: null }])
  const [saving, setSaving] = useState(false)

  const ingOf = (id: number | null) => ingredients.find((i) => i.id === id)

  const setLine = (key: number, patch: Partial<DraftLine>) =>
    setLines((ls) => ls.map((l) => (l.key === key ? { ...l, ...patch } : l)))

  const addLine = () =>
    setLines((ls) => [...ls, { key: Math.max(0, ...ls.map((l) => l.key)) + 1, ingredient_id: null, qty: null, unit: 'g', unit_price: null }])

  const total = lines.reduce((s, l) => s + (l.qty ?? 0) * (l.unit_price ?? 0), 0)

  const submit = async () => {
    const valid = lines.filter((l) => l.ingredient_id && (l.qty ?? 0) > 0)
    if (!valid.length) {
      message.warning('Phiếu cần ít nhất 1 dòng có nguyên liệu và số lượng')
      return
    }
    setSaving(true)
    try {
      const r = await api.purchaseCreate({
        supplier,
        date,
        note,
        lines: valid.map((l) => ({
          ingredient_id: l.ingredient_id!,
          qty: l.qty!,
          unit: l.unit,
          unit_price: l.unit_price ?? 0,
        })),
      })
      if (r.error) {
        message.error(r.error)
        return
      }
      message.success(`Đã tạo phiếu ${r.purchase?.code} — ${fmtMoney(r.purchase?.total ?? 0)}`)
      setSupplier('')
      setNote('')
      setLines([{ key: 1, ingredient_id: null, qty: null, unit: 'g', unit_price: null }])
      onCreated()
    } finally {
      setSaving(false)
    }
  }

  return (
    <Card size="small" title="Tạo phiếu nhập hàng">
      <Flex vertical gap={8}>
        <Flex gap={8} wrap>
          <Input style={{ width: 220 }} placeholder="Nhà cung cấp" value={supplier} onChange={(e) => setSupplier(e.target.value)} />
          <Input style={{ width: 140 }} placeholder="YYYY-MM-DD" value={date} onChange={(e) => setDate(e.target.value)} />
          <Input style={{ flex: 1, minWidth: 180 }} placeholder="Ghi chú" value={note} onChange={(e) => setNote(e.target.value)} />
        </Flex>
        {lines.map((l) => {
          const ing = ingOf(l.ingredient_id)
          const units = ing ? PURCHASE_UNITS[ing.unit] ?? [ing.unit] : ['g']
          return (
            <Flex key={l.key} gap={8} align="center" wrap>
              <Select
                showSearch
                style={{ width: 240 }}
                placeholder="Nguyên liệu…"
                optionFilterProp="label"
                value={l.ingredient_id}
                options={ingredients.map((i) => ({ value: i.id, label: `${i.name} (${i.unit})` }))}
                onChange={(id) => {
                  const ni = ingredients.find((i) => i.id === id)
                  setLine(l.key, { ingredient_id: Number(id), unit: ni ? ni.unit : 'g' })
                }}
              />
              <InputNumber style={{ width: 110 }} min={0} placeholder="SL" value={l.qty} onChange={(v) => setLine(l.key, { qty: v === null ? null : Number(v) })} />
              <Select style={{ width: 84 }} value={l.unit} options={units.map((u) => ({ value: u, label: u }))} onChange={(u) => setLine(l.key, { unit: u })} />
              <InputNumber
                style={{ width: 150 }}
                min={0}
                step={1000}
                placeholder="Đơn giá"
                value={l.unit_price}
                addonAfter={`đ/${l.unit}`}
                onChange={(v) => setLine(l.key, { unit_price: v === null ? null : Number(v) })}
              />
              <Text style={{ width: 110, textAlign: 'right' }}>{fmtMoney((l.qty ?? 0) * (l.unit_price ?? 0))}</Text>
              <Button size="small" danger icon={<DeleteOutlined />} onClick={() => setLines((ls) => ls.filter((x) => x.key !== l.key))} />
            </Flex>
          )
        })}
        <Flex justify="space-between" align="center">
          <Button size="small" icon={<PlusOutlined />} onClick={addLine}>
            Thêm dòng
          </Button>
          <Flex gap={12} align="center">
            <Text strong>Tổng: {fmtMoney(total)}</Text>
            <Button type="primary" loading={saving} onClick={submit}>
              Ghi phiếu nhập
            </Button>
          </Flex>
        </Flex>
      </Flex>
    </Card>
  )
}

function PurchaseList({ reloadKey }: { reloadKey: number }) {
  const [items, setItems] = useState<Purchase[]>([])
  const [loading, setLoading] = useState(true)
  const [detail, setDetail] = useState<Purchase | null>(null)

  const load = () => {
    setLoading(true)
    api
      .purchases({ limit: 30 })
      .then((r) => setItems(r.purchases))
      .finally(() => setLoading(false))
  }
  useEffect(load, [reloadKey])

  const openDetail = async (id: number) => {
    const r = await api.purchaseGet(id)
    if (r.purchase) setDetail(r.purchase)
  }

  return (
    <Card size="small" title="Phiếu nhập gần đây" extra={<Button size="small" icon={<ReloadOutlined />} onClick={load} />}>
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        onRow={(r) => ({ onClick: () => openDetail(r.id), style: { cursor: 'pointer' } })}
        locale={{ emptyText: 'Chưa có phiếu nhập' }}
        columns={[
          { title: 'Mã', dataIndex: 'code' },
          { title: 'Ngày', dataIndex: 'purchase_date', render: fmtDate },
          { title: 'Nhà cung cấp', dataIndex: 'supplier', render: (s) => s || '—' },
          { title: 'Số dòng', dataIndex: 'line_count', align: 'right' },
          { title: 'Tổng tiền', dataIndex: 'total', align: 'right', render: fmtMoney },
          { title: 'Ghi chú', dataIndex: 'note', ellipsis: true },
        ]}
      />
      <Modal open={!!detail} onCancel={() => setDetail(null)} footer={null} title={detail?.code} width={640}>
        {detail && (
          <Flex vertical gap={8}>
            <Text type="secondary">
              Ngày {fmtDate(detail.purchase_date)}
              {detail.supplier ? ` · ${detail.supplier}` : ''}
              {detail.note ? ` · ${detail.note}` : ''}
            </Text>
            <Table
              size="small"
              rowKey="id"
              dataSource={detail.lines ?? []}
              pagination={false}
              columns={[
                { title: 'Nguyên liệu', dataIndex: 'name' },
                {
                  title: 'SL nhập',
                  key: 'qin',
                  align: 'right',
                  render: (_, r) => `${fmtQty(r.qty_input)} ${r.unit_input}`,
                },
                {
                  title: 'Quy đổi',
                  key: 'qbase',
                  align: 'right',
                  render: (_, r) => `${fmtQty(r.qty)} ${r.unit}`,
                },
                {
                  title: 'Đơn giá',
                  key: 'price',
                  align: 'right',
                  render: (_, r) => `${fmtMoney(r.unit_price)}/${r.unit_input}`,
                },
                { title: 'Thành tiền', dataIndex: 'amount', align: 'right', render: fmtMoney },
              ]}
            />
            <Flex justify="end">
              <Text strong>Tổng: {fmtMoney(detail.total)}</Text>
            </Flex>
          </Flex>
        )}
      </Modal>
    </Card>
  )
}

function PurchaseReportCard() {
  const [from, setFrom] = useState(todayISO().slice(0, 8) + '01')
  const [to, setTo] = useState(todayISO())
  const [groupBy, setGroupBy] = useState('ingredient')
  const [report, setReport] = useState<PurchaseReport | null>(null)
  const [loading, setLoading] = useState(false)

  const load = () => {
    setLoading(true)
    api
      .purchaseReport({ from, to, group_by: groupBy })
      .then(setReport)
      .finally(() => setLoading(false))
  }
  useEffect(load, [groupBy])

  const columns =
    groupBy === 'supplier'
      ? [
          { title: 'Nhà cung cấp', dataIndex: 'supplier' },
          { title: 'Số phiếu', dataIndex: 'purchase_count', align: 'right' as const },
          { title: 'Tiền nhập', dataIndex: 'amount', align: 'right' as const, render: fmtMoney },
        ]
      : groupBy === 'day'
        ? [
            { title: 'Ngày', dataIndex: 'date', render: fmtDate },
            { title: 'Số phiếu', dataIndex: 'purchase_count', align: 'right' as const },
            { title: 'Tiền nhập', dataIndex: 'amount', align: 'right' as const, render: fmtMoney },
          ]
        : [
            { title: 'Nguyên liệu', dataIndex: 'ingredient' },
            { title: 'SL nhập', dataIndex: 'qty_display', align: 'right' as const },
            { title: 'Tiền nhập', dataIndex: 'amount', align: 'right' as const, render: fmtMoney },
          ]

  return (
    <Card
      size="small"
      title="Báo cáo nhập hàng"
      extra={
        <Segmented
          size="small"
          value={groupBy}
          onChange={(v) => setGroupBy(String(v))}
          options={[
            { value: 'ingredient', label: 'Theo nguyên liệu' },
            { value: 'supplier', label: 'Theo NCC' },
            { value: 'day', label: 'Theo ngày' },
          ]}
        />
      }
    >
      <Flex gap={8} style={{ marginBottom: 8 }} wrap>
        <Input style={{ width: 130 }} value={from} onChange={(e) => setFrom(e.target.value)} placeholder="Từ YYYY-MM-DD" />
        <Input style={{ width: 130 }} value={to} onChange={(e) => setTo(e.target.value)} placeholder="Đến YYYY-MM-DD" />
        <Button onClick={load} loading={loading}>
          Xem
        </Button>
        {report && (
          <Text type="secondary" style={{ alignSelf: 'center' }}>
            {report.purchase_count} phiếu · tổng {fmtMoney(report.total_amount)}
          </Text>
        )}
      </Flex>
      {report ? (
        <Table size="small" rowKey={(_, i) => String(i)} loading={loading} dataSource={report.rows} pagination={{ pageSize: 12, hideOnSinglePage: true }} columns={columns} locale={{ emptyText: 'Không có phiếu trong khoảng này' }} />
      ) : (
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} />
      )}
    </Card>
  )
}
