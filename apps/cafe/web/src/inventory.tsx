import { useEffect, useState } from 'react'
import {
  Badge,
  Button,
  Card,
  Checkbox,
  Drawer,
  Flex,
  Form,
  Input,
  InputNumber,
  Modal,
  Radio,
  Select,
  Switch,
  Table,
  Tag,
  Typography,
  message,
} from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  api,
  BASE_UNITS,
  fmtDate,
  fmtMoney,
  fmtQty,
  MOVE_KIND_COLORS,
  MOVE_KIND_LABELS,
  type Ingredient,
  type StockCard,
} from './api'

const { Text } = Typography

export function InventoryTab({ onChange }: { onChange: () => void }) {
  const [items, setItems] = useState<Ingredient[]>([])
  const [loading, setLoading] = useState(true)
  const [q, setQ] = useState('')
  const [lowOnly, setLowOnly] = useState(false)
  const [showInactive, setShowInactive] = useState(false)
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<Ingredient | null>(null)
  const [adjusting, setAdjusting] = useState<Ingredient | null>(null)
  const [cardOf, setCardOf] = useState<Ingredient | null>(null)

  const load = () => {
    setLoading(true)
    api
      .ingredients({ q: q || undefined, low_only: lowOnly, include_inactive: showInactive })
      .then((r) => setItems(r.ingredients))
      .finally(() => setLoading(false))
  }
  useEffect(load, [q, lowOnly, showInactive])

  const saved = () => {
    load()
    onChange()
  }

  return (
    <Card
      size="small"
      title={
        <Flex gap={8} align="center" wrap>
          <Input.Search allowClear placeholder="Tìm nguyên liệu…" style={{ width: 220 }} onSearch={setQ} />
          <Checkbox checked={lowOnly} onChange={(e) => setLowOnly(e.target.checked)}>
            Chỉ hàng sắp hết
          </Checkbox>
          <Checkbox checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)}>
            Hiện ngừng dùng
          </Checkbox>
        </Flex>
      }
      extra={
        <Flex gap={8}>
          <Button size="small" icon={<ReloadOutlined />} onClick={load} />
          <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
            Thêm nguyên liệu
          </Button>
        </Flex>
      }
    >
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        onRow={(r) => ({ onClick: () => setCardOf(r), style: { cursor: 'pointer' } })}
        columns={[
          {
            title: 'Nguyên liệu',
            dataIndex: 'name',
            render: (n, r) => (
              <Flex align="center" gap={6}>
                <Text delete={r.status === 'inactive'}>{n}</Text>
                {r.low_stock && <Badge status="error" title="Dưới tồn tối thiểu" />}
              </Flex>
            ),
          },
          {
            title: 'Tồn kho',
            dataIndex: 'stock_display',
            align: 'right',
            render: (v, r) => (
              <Text style={{ color: r.stock < 0 ? '#f5222d' : r.low_stock ? '#faad14' : undefined }}>{v}</Text>
            ),
          },
          {
            title: 'Tồn tối thiểu',
            dataIndex: 'min_stock',
            align: 'right',
            render: (v, r) => (v > 0 ? `${fmtQty(v)} ${r.unit}` : '—'),
          },
          {
            title: 'Giá vốn BQ',
            dataIndex: 'avg_cost',
            align: 'right',
            render: (v, r) => `${fmtMoney(v)}/${r.unit}`,
          },
          { title: 'Giá trị tồn', dataIndex: 'stock_value', align: 'right', render: fmtMoney },
          {
            title: 'Còn đủ',
            dataIndex: 'days_left',
            align: 'right',
            render: (v) => (v === null || v === undefined ? '—' : `~${v} ngày`),
          },
          {
            title: '',
            key: 'actions',
            render: (_, r) => (
              <Flex gap={6}>
                <Button
                  size="small"
                  onClick={(e) => {
                    e.stopPropagation()
                    setEditing(r)
                  }}
                >
                  Sửa
                </Button>
                <Button
                  size="small"
                  onClick={(e) => {
                    e.stopPropagation()
                    setAdjusting(r)
                  }}
                >
                  Kiểm kê
                </Button>
              </Flex>
            ),
          },
        ]}
      />

      <IngredientFormModal
        open={adding || !!editing}
        item={editing}
        onClose={() => {
          setAdding(false)
          setEditing(null)
        }}
        onSaved={() => {
          setAdding(false)
          setEditing(null)
          saved()
        }}
      />
      <AdjustModal item={adjusting} onClose={() => setAdjusting(null)} onSaved={() => { setAdjusting(null); saved() }} />
      <CardDrawer item={cardOf} onClose={() => setCardOf(null)} />
    </Card>
  )
}

function IngredientFormModal({
  open,
  item,
  onClose,
  onSaved,
}: {
  open: boolean
  item: Ingredient | null
  onClose: () => void
  onSaved: () => void
}) {
  const [form] = Form.useForm()

  useEffect(() => {
    if (open) {
      form.setFieldsValue(
        item
          ? { name: item.name, unit: item.unit, min_stock: item.min_stock, note: item.note, active: item.status === 'active' }
          : { name: '', unit: 'g', min_stock: 0, note: '', active: true },
      )
    }
  }, [open, item, form])

  const submit = async () => {
    const v = await form.validateFields()
    const r = item
      ? await api.ingredientUpdate(item.id, v)
      : await api.ingredientAdd({ name: v.name, unit: v.unit, min_stock: v.min_stock ?? 0, note: v.note ?? '' })
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success(item ? 'Đã cập nhật nguyên liệu' : 'Đã thêm nguyên liệu')
    onSaved()
  }

  return (
    <Modal open={open} onCancel={onClose} onOk={submit} title={item ? `Sửa: ${item.name}` : 'Thêm nguyên liệu'} okText="Lưu">
      <Form form={form} layout="vertical">
        <Form.Item name="name" label="Tên nguyên liệu" rules={[{ required: true, message: 'Nhập tên' }]}>
          <Input placeholder="Cà phê bột, Sữa đặc, Ly nhựa…" />
        </Form.Item>
        <Form.Item
          name="unit"
          label="Đơn vị gốc (tồn kho & công thức tính theo đơn vị này)"
          rules={[{ required: true }]}
          extra="Nhập hàng vẫn khai được kg / lít — app tự quy đổi. Đã có biến động kho thì không đổi được nữa."
        >
          <Select options={BASE_UNITS.map((u) => ({ value: u, label: u }))} />
        </Form.Item>
        <Form.Item name="min_stock" label="Tồn tối thiểu (cảnh báo sắp hết, theo đơn vị gốc)">
          <InputNumber style={{ width: '100%' }} min={0} />
        </Form.Item>
        <Form.Item name="note" label="Ghi chú">
          <Input />
        </Form.Item>
        {item && (
          <Form.Item name="active" label="Đang dùng" valuePropName="checked">
            <Switch />
          </Form.Item>
        )}
      </Form>
    </Modal>
  )
}

function AdjustModal({
  item,
  onClose,
  onSaved,
}: {
  item: Ingredient | null
  onClose: () => void
  onSaved: () => void
}) {
  const [mode, setMode] = useState<'set' | 'delta'>('set')
  const [value, setValue] = useState<number | null>(null)
  const [reason, setReason] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (item) {
      setMode('set')
      setValue(item.stock)
      setReason('kiểm kê')
    }
  }, [item])

  const submit = async () => {
    if (!item || value === null) return
    setSaving(true)
    try {
      const r = await api.stockAdjust({
        ingredient_id: item.id,
        delta: mode === 'delta' ? value : undefined,
        set_qty: mode === 'set' ? value : undefined,
        reason,
      })
      if (r.error) {
        message.error(r.error)
        return
      }
      message.success(`Đã điều chỉnh — tồn mới: ${r.stock_display}`)
      onSaved()
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal
      open={!!item}
      onCancel={onClose}
      onOk={submit}
      okButtonProps={{ loading: saving }}
      title={item ? `Kiểm kê: ${item.name} (đang ${item.stock_display})` : ''}
      okText="Ghi điều chỉnh"
    >
      <Flex vertical gap={12}>
        <Radio.Group
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          options={[
            { value: 'set', label: 'Đặt tồn = số đếm thực tế' },
            { value: 'delta', label: 'Cộng/trừ chênh lệch (±)' },
          ]}
        />
        <InputNumber
          style={{ width: '100%' }}
          value={value}
          onChange={(v) => setValue(v === null ? null : Number(v))}
          addonAfter={item?.unit}
          placeholder={mode === 'set' ? 'Số đếm được' : 'Chênh lệch, âm = thiếu'}
        />
        <Input placeholder="Lý do (đợt kiểm kê, rơi vãi, hết hạn…)" value={reason} onChange={(e) => setReason(e.target.value)} />
      </Flex>
    </Modal>
  )
}

function CardDrawer({ item, onClose }: { item: Ingredient | null; onClose: () => void }) {
  const [card, setCard] = useState<StockCard | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (item) {
      setLoading(true)
      api
        .stockCard(item.id)
        .then(setCard)
        .finally(() => setLoading(false))
    } else {
      setCard(null)
    }
  }, [item])

  return (
    <Drawer open={!!item} onClose={onClose} width={640} title={item ? `Thẻ kho: ${item.name}` : ''}>
      {card && !card.error && (
        <Flex vertical gap={8}>
          <Text type="secondary">
            Dư đầu: {fmtQty(card.opening ?? 0)} {card.ingredient?.unit} · Dư cuối: {fmtQty(card.closing ?? 0)}{' '}
            {card.ingredient?.unit}
          </Text>
          <Table
            size="small"
            rowKey={(r) => `${r.date}-${r.kind}-${r.balance}-${r.ref}`}
            loading={loading}
            dataSource={card.rows ?? []}
            pagination={{ pageSize: 20, hideOnSinglePage: true }}
            columns={[
              { title: 'Ngày', dataIndex: 'date', render: fmtDate },
              {
                title: 'Loại',
                dataIndex: 'kind',
                render: (k) => <Tag color={MOVE_KIND_COLORS[k]}>{MOVE_KIND_LABELS[k] ?? k}</Tag>,
              },
              {
                title: 'SL (±)',
                dataIndex: 'qty',
                align: 'right',
                render: (v) => <Text style={{ color: v >= 0 ? '#10b981' : '#f5222d' }}>{fmtQty(v)}</Text>,
              },
              { title: 'Dư', dataIndex: 'balance', align: 'right', render: fmtQty },
              { title: 'Chứng từ', dataIndex: 'ref' },
              { title: 'Ghi chú', dataIndex: 'note', ellipsis: true },
            ]}
          />
        </Flex>
      )}
    </Drawer>
  )
}
