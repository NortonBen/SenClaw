import { useEffect, useMemo, useState } from 'react'
import {
  Alert,
  Button,
  Card,
  Checkbox,
  DatePicker,
  Descriptions,
  Drawer,
  Empty,
  Flex,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Table,
  Tag,
  Typography,
  message,
} from 'antd'
import { DeleteOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  api,
  fmtMoney,
  fmtQty,
  MOVE_KIND_COLORS,
  MOVE_KIND_LABELS,
  PARTNER_KIND_LABELS,
  type CardRow,
  type Move,
  type Partner,
  type Product,
  type StockRow,
  type Warehouse,
} from './api'

const { Text } = Typography

const moneyInput = {
  formatter: (v: any) => `${v}`.replace(/\B(?=(\d{3})+(?!\d))/g, ','),
  parser: (v: any) => `${v}`.replace(/,/g, '') as any,
}

// ---- Sản phẩm ----

export function ProductsTab({ onChange }: { onChange: () => void }) {
  const [items, setItems] = useState<Product[]>([])
  const [loading, setLoading] = useState(true)
  const [q, setQ] = useState('')
  const [lowOnly, setLowOnly] = useState(false)
  const [editing, setEditing] = useState<Product | null>(null)
  const [adding, setAdding] = useState(false)
  const [detail, setDetail] = useState<Product | null>(null)

  const load = () => {
    setLoading(true)
    api
      .products({ q: q || undefined, low_stock: lowOnly })
      .then((r) => setItems(r.products))
      .finally(() => setLoading(false))
  }
  useEffect(load, [q, lowOnly])

  return (
    <Card
      size="small"
      title={
        <Flex gap={8} align="center">
          <Input.Search allowClear placeholder="Tìm tên / SKU / barcode…" style={{ width: 240 }} onSearch={setQ} />
          <Checkbox checked={lowOnly} onChange={(e) => setLowOnly(e.target.checked)}>
            Chỉ hàng sắp hết
          </Checkbox>
        </Flex>
      }
      extra={
        <Space>
          <Button icon={<ReloadOutlined />} onClick={load} />
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
            Thêm sản phẩm
          </Button>
        </Space>
      }
    >
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        onRow={(r) => ({ onClick: () => setDetail(r), style: { cursor: 'pointer' } })}
        columns={[
          { title: 'SKU', dataIndex: 'sku', width: 100, render: (v: string) => v || '—' },
          {
            title: 'Tên',
            dataIndex: 'name',
            render: (v: string, r: Product) => (
              <Space size={6}>
                {v}
                {r.status === 'inactive' && <Tag>ngừng bán</Tag>}
                {r.low_stock && <Tag color="orange">sắp hết</Tag>}
              </Space>
            ),
          },
          { title: 'Nhóm', dataIndex: 'category', width: 120, render: (v: string) => v || '—' },
          {
            title: 'Tồn',
            dataIndex: 'on_hand',
            align: 'right' as const,
            width: 110,
            render: (v: number, r: Product) => (
              <Text style={{ color: r.low_stock ? '#fa8c16' : v <= 0 ? '#f5222d' : undefined }}>
                {fmtQty(v)} {r.unit}
              </Text>
            ),
          },
          { title: 'Giá vốn BQ', dataIndex: 'avg_cost', align: 'right' as const, width: 120, render: (v: number) => fmtMoney(v) },
          { title: 'Giá bán', dataIndex: 'sell_price', align: 'right' as const, width: 120, render: (v: number) => fmtMoney(v) },
          { title: 'Giá trị tồn', dataIndex: 'stock_value', align: 'right' as const, width: 130, render: (v: number) => fmtMoney(v) },
          {
            title: '',
            width: 60,
            render: (_: any, r: Product) => (
              <Button size="small" onClick={(e) => { e.stopPropagation(); setEditing(r) }}>
                Sửa
              </Button>
            ),
          },
        ]}
      />
      <ProductForm
        open={adding || !!editing}
        product={editing}
        onClose={() => {
          setAdding(false)
          setEditing(null)
        }}
        onSaved={() => {
          setAdding(false)
          setEditing(null)
          load()
          onChange()
        }}
      />
      <ProductDetail product={detail} onClose={() => setDetail(null)} />
    </Card>
  )
}

function ProductForm({
  open,
  product,
  onClose,
  onSaved,
}: {
  open: boolean
  product: Product | null
  onClose: () => void
  onSaved: () => void
}) {
  const [form] = Form.useForm()
  useEffect(() => {
    if (open) {
      form.resetFields()
      if (product) form.setFieldsValue(product)
    }
  }, [open, product])

  const submit = async () => {
    const v = await form.validateFields()
    const r = product ? await api.productUpdate(product.id, v) : await api.productAdd(v)
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success(product ? 'Đã cập nhật sản phẩm' : 'Đã thêm sản phẩm')
    onSaved()
  }

  return (
    <Modal
      title={product ? `Sửa sản phẩm #${product.id}` : 'Thêm sản phẩm'}
      open={open}
      onCancel={onClose}
      onOk={submit}
      okText={product ? 'Lưu' : 'Thêm'}
      cancelText="Huỷ"
      destroyOnHidden
    >
      <Form form={form} layout="vertical">
        <Flex gap={8}>
          <Form.Item name="name" label="Tên" rules={[{ required: true, message: 'Nhập tên sản phẩm' }]} style={{ flex: 2 }}>
            <Input />
          </Form.Item>
          <Form.Item name="sku" label="SKU" style={{ flex: 1 }}>
            <Input />
          </Form.Item>
        </Flex>
        <Flex gap={8}>
          <Form.Item name="unit" label="Đơn vị" style={{ flex: 1 }}>
            <Input placeholder="cái" />
          </Form.Item>
          <Form.Item name="category" label="Nhóm hàng" style={{ flex: 1 }}>
            <Input />
          </Form.Item>
          <Form.Item name="barcode" label="Barcode" style={{ flex: 1 }}>
            <Input />
          </Form.Item>
        </Flex>
        <Flex gap={8}>
          <Form.Item name="cost_price" label="Giá vốn" style={{ flex: 1 }}>
            <InputNumber min={0} style={{ width: '100%' }} {...moneyInput} />
          </Form.Item>
          <Form.Item name="sell_price" label="Giá bán" style={{ flex: 1 }}>
            <InputNumber min={0} style={{ width: '100%' }} {...moneyInput} />
          </Form.Item>
          <Form.Item name="min_stock" label="Tồn tối thiểu" style={{ flex: 1 }} tooltip="Cảnh báo khi tồn dưới mức này (0 = không cảnh báo)">
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
        </Flex>
        {product && (
          <Form.Item name="status" label="Trạng thái">
            <Select
              options={[
                { value: 'active', label: 'Đang bán' },
                { value: 'inactive', label: 'Ngừng bán' },
              ]}
            />
          </Form.Item>
        )}
        <Form.Item name="note" label="Ghi chú">
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

function ProductDetail({ product, onClose }: { product: Product | null; onClose: () => void }) {
  const [byWh, setByWh] = useState<StockRow[]>([])
  const [card, setCard] = useState<CardRow[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!product) return
    setLoading(true)
    api
      .productGet(product.id)
      .then((r) => {
        setByWh(r.by_warehouse ?? [])
        setCard(r.card ?? [])
      })
      .finally(() => setLoading(false))
  }, [product])

  return (
    <Drawer open={!!product} onClose={onClose} width={640} title={product ? `${product.name}${product.sku ? ` (${product.sku})` : ''}` : ''}>
      {loading ? (
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      ) : (
        product && (
          <Space direction="vertical" size="large" style={{ width: '100%' }}>
            <Descriptions
              size="small"
              column={2}
              items={[
                { label: 'Tồn tổng', children: `${fmtQty(product.on_hand)} ${product.unit}` },
                { label: 'Giá trị tồn', children: fmtMoney(product.stock_value) },
                { label: 'Giá vốn BQ', children: fmtMoney(product.avg_cost) },
                { label: 'Giá bán', children: fmtMoney(product.sell_price) },
              ]}
            />
            <Card size="small" title="Tồn theo kho">
              {byWh.length ? (
                <Table
                  size="small"
                  rowKey={(r) => `${r.warehouse_id}`}
                  pagination={false}
                  dataSource={byWh}
                  columns={[
                    { title: 'Kho', dataIndex: 'warehouse_name' },
                    { title: 'Tồn', dataIndex: 'qty', align: 'right' as const, render: (v: number) => fmtQty(v) },
                    { title: 'Giá trị', dataIndex: 'value', align: 'right' as const, render: (v: number) => fmtMoney(v) },
                  ]}
                />
              ) : (
                <Empty description="Chưa có tồn" image={Empty.PRESENTED_IMAGE_SIMPLE} />
              )}
            </Card>
            <Card size="small" title="Thẻ kho gần đây (toàn công ty)">
              <CardTable rows={card} />
            </Card>
          </Space>
        )
      )}
    </Drawer>
  )
}

// ---- Phiếu kho ----

export function MovesTab({ onChange }: { onChange: () => void }) {
  const [items, setItems] = useState<Move[]>([])
  const [loading, setLoading] = useState(true)
  const [kind, setKind] = useState<string | undefined>()
  const [creating, setCreating] = useState(false)
  const [detail, setDetail] = useState<Move | null>(null)

  const load = () => {
    setLoading(true)
    api
      .moves({ kind })
      .then((r) => setItems(r.moves))
      .finally(() => setLoading(false))
  }
  useEffect(load, [kind])

  const del = async (id: number) => {
    const r = await api.moveDelete(id)
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success(`Đã xoá phiếu ${r.deleted}`)
    load()
    onChange()
  }

  return (
    <Card
      size="small"
      title={
        <Select
          allowClear
          placeholder="Tất cả loại phiếu"
          style={{ width: 180 }}
          value={kind}
          onChange={setKind}
          options={Object.entries(MOVE_KIND_LABELS).map(([value, label]) => ({ value, label }))}
        />
      }
      extra={
        <Space>
          <Button icon={<ReloadOutlined />} onClick={load} />
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreating(true)}>
            Tạo phiếu
          </Button>
        </Space>
      }
    >
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        onRow={(r) => ({ onClick: () => setDetail(r), style: { cursor: 'pointer' } })}
        columns={[
          { title: 'Mã', dataIndex: 'code', width: 95 },
          {
            title: 'Loại',
            dataIndex: 'kind',
            width: 115,
            render: (k: string) => <Tag color={MOVE_KIND_COLORS[k]}>{MOVE_KIND_LABELS[k] ?? k}</Tag>,
          },
          { title: 'Ngày', dataIndex: 'move_date', width: 105 },
          {
            title: 'Kho',
            render: (_: any, r: Move) => (r.kind === 'transfer' ? `${r.warehouse_name} → ${r.to_warehouse_name}` : r.warehouse_name),
          },
          { title: 'Đối tác', dataIndex: 'partner_name', ellipsis: true, render: (v: string | null) => v || '—' },
          { title: 'SL', dataIndex: 'total_qty', align: 'right' as const, width: 80, render: (v: number) => fmtQty(v) },
          { title: 'Giá trị', dataIndex: 'total_value', align: 'right' as const, width: 120, render: (v: number) => fmtMoney(v) },
          {
            title: '',
            width: 50,
            render: (_: any, r: Move) => (
              <Popconfirm title={`Xoá phiếu ${r.code}?`} okText="Xoá" cancelText="Huỷ" onConfirm={() => del(r.id)}>
                <Button size="small" danger icon={<DeleteOutlined />} onClick={(e) => e.stopPropagation()} />
              </Popconfirm>
            ),
          },
        ]}
      />
      <MoveForm
        open={creating}
        onClose={() => setCreating(false)}
        onSaved={() => {
          setCreating(false)
          load()
          onChange()
        }}
      />
      <MoveDetail move={detail} onClose={() => setDetail(null)} />
    </Card>
  )
}

function MoveForm({ open, onClose, onSaved }: { open: boolean; onClose: () => void; onSaved: () => void }) {
  const [form] = Form.useForm()
  const [products, setProducts] = useState<Product[]>([])
  const [warehouses, setWarehouses] = useState<Warehouse[]>([])
  const [partners, setPartners] = useState<Partner[]>([])
  const kind = Form.useWatch('kind', form) ?? 'receipt'

  useEffect(() => {
    if (!open) return
    form.resetFields()
    api.products({ status: 'active' }).then((r) => setProducts(r.products))
    api.warehouses('active').then((r) => setWarehouses(r.warehouses))
    api.partners().then((r) => setPartners(r.partners))
  }, [open])

  const productOpts = useMemo(
    () =>
      products.map((p) => ({
        value: p.id,
        label: `${p.name}${p.sku ? ` (${p.sku})` : ''} — tồn ${fmtQty(p.on_hand)} ${p.unit}`,
        product: p,
      })),
    [products],
  )

  const submit = async () => {
    const v = await form.validateFields()
    const r = await api.moveCreate({
      kind: v.kind,
      warehouse_id: v.warehouse_id,
      to_warehouse_id: v.to_warehouse_id,
      partner_id: v.partner_id,
      move_date: v.move_date ? v.move_date.format('YYYY-MM-DD') : undefined,
      note: v.note,
      lines: (v.lines ?? []).map((l: any) => ({ product_id: l.product_id, qty: l.qty, unit_price: l.unit_price ?? 0 })),
    })
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success(`Đã tạo phiếu ${r.move?.code}`)
    onSaved()
  }

  const priceLabel = kind === 'receipt' ? 'Giá nhập' : kind === 'issue' ? 'Giá xuất' : 'Đơn giá'

  return (
    <Modal
      title="Tạo phiếu kho"
      open={open}
      onCancel={onClose}
      onOk={submit}
      okText="Tạo phiếu"
      cancelText="Huỷ"
      width={760}
      destroyOnHidden
    >
      <Form form={form} layout="vertical" initialValues={{ kind: 'receipt', lines: [{}] }}>
        <Flex gap={8}>
          <Form.Item name="kind" label="Loại phiếu" style={{ flex: 1 }}>
            <Select options={Object.entries(MOVE_KIND_LABELS).map(([value, label]) => ({ value, label }))} />
          </Form.Item>
          <Form.Item name="warehouse_id" label={kind === 'transfer' ? 'Kho đi' : 'Kho'} rules={[{ required: true, message: 'Chọn kho' }]} style={{ flex: 1 }}>
            <Select options={warehouses.map((w) => ({ value: w.id, label: w.name }))} />
          </Form.Item>
          {kind === 'transfer' && (
            <Form.Item name="to_warehouse_id" label="Kho đến" rules={[{ required: true, message: 'Chọn kho đến' }]} style={{ flex: 1 }}>
              <Select options={warehouses.map((w) => ({ value: w.id, label: w.name }))} />
            </Form.Item>
          )}
          {(kind === 'receipt' || kind === 'issue') && (
            <Form.Item name="partner_id" label={kind === 'receipt' ? 'Nhà cung cấp' : 'Khách hàng'} style={{ flex: 1 }}>
              <Select
                allowClear
                options={partners
                  .filter((p) => (kind === 'receipt' ? p.kind !== 'customer' : p.kind !== 'supplier'))
                  .map((p) => ({ value: p.id, label: p.name }))}
              />
            </Form.Item>
          )}
          <Form.Item name="move_date" label="Ngày" style={{ width: 150 }}>
            <DatePicker style={{ width: '100%' }} />
          </Form.Item>
        </Flex>
        {kind === 'adjust' && (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 12 }}
            message="Phiếu điều chỉnh: số lượng là DELTA có dấu — kiểm kê thừa nhập số dương, thiếu nhập số âm."
          />
        )}
        <Form.List name="lines">
          {(fields, { add, remove }) => (
            <>
              {fields.map((field) => (
                <Flex key={field.key} gap={8} align="start">
                  <Form.Item name={[field.name, 'product_id']} rules={[{ required: true, message: 'Chọn sản phẩm' }]} style={{ flex: 2 }}>
                    <Select showSearch optionFilterProp="label" placeholder="Sản phẩm" options={productOpts} />
                  </Form.Item>
                  <Form.Item name={[field.name, 'qty']} rules={[{ required: true, message: 'Nhập SL' }]} style={{ width: 120 }}>
                    <InputNumber placeholder="Số lượng" style={{ width: '100%' }} />
                  </Form.Item>
                  <Form.Item name={[field.name, 'unit_price']} style={{ width: 160 }}>
                    <InputNumber min={0} placeholder={priceLabel} style={{ width: '100%' }} {...moneyInput} />
                  </Form.Item>
                  <Button icon={<DeleteOutlined />} onClick={() => remove(field.name)} disabled={fields.length <= 1} />
                </Flex>
              ))}
              <Button type="dashed" icon={<PlusOutlined />} onClick={() => add({})} block>
                Thêm dòng hàng
              </Button>
            </>
          )}
        </Form.List>
        <Form.Item name="note" label="Ghi chú" style={{ marginTop: 12 }}>
          <Input />
        </Form.Item>
      </Form>
    </Modal>
  )
}

function MoveDetail({ move, onClose }: { move: Move | null; onClose: () => void }) {
  const [full, setFull] = useState<Move | null>(null)

  useEffect(() => {
    if (!move) {
      setFull(null)
      return
    }
    api.moveGet(move.id).then((r) => setFull(r.move ?? null))
  }, [move])

  return (
    <Drawer open={!!move} onClose={onClose} width={620} title={move ? `Phiếu ${move.code}` : ''}>
      {!full ? (
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      ) : (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Descriptions
            size="small"
            column={2}
            items={[
              { label: 'Loại', children: <Tag color={MOVE_KIND_COLORS[full.kind]}>{MOVE_KIND_LABELS[full.kind]}</Tag> },
              { label: 'Ngày', children: full.move_date },
              {
                label: 'Kho',
                children: full.kind === 'transfer' ? `${full.warehouse_name} → ${full.to_warehouse_name}` : full.warehouse_name,
              },
              { label: 'Đối tác', children: full.partner_name || '—' },
              { label: 'Ghi chú', children: full.note || '—', span: 2 },
            ]}
          />
          <Table
            size="small"
            rowKey="id"
            pagination={false}
            dataSource={full.lines ?? []}
            columns={[
              { title: 'Sản phẩm', dataIndex: 'product_name', render: (v: string, r) => `${v}${r.sku ? ` (${r.sku})` : ''}` },
              { title: 'SL', dataIndex: 'qty', align: 'right' as const, render: (v: number, r) => `${fmtQty(v)} ${r.unit}` },
              { title: 'Đơn giá', dataIndex: 'unit_price', align: 'right' as const, render: (v: number) => fmtMoney(v) },
              { title: 'Thành tiền', dataIndex: 'amount', align: 'right' as const, render: (v: number) => fmtMoney(v) },
            ]}
            summary={() => (
              <Table.Summary.Row>
                <Table.Summary.Cell index={0}>
                  <Text strong>Tổng</Text>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={1} align="right">
                  <Text strong>{fmtQty(full.total_qty)}</Text>
                </Table.Summary.Cell>
                <Table.Summary.Cell index={2} />
                <Table.Summary.Cell index={3} align="right">
                  <Text strong>{fmtMoney(full.total_value)}</Text>
                </Table.Summary.Cell>
              </Table.Summary.Row>
            )}
          />
        </Space>
      )}
    </Drawer>
  )
}

// ---- Thẻ kho ----

function CardTable({ rows }: { rows: CardRow[] }) {
  if (!rows.length) return <Empty description="Chưa có chứng từ" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  return (
    <Table
      size="small"
      rowKey={(r) => `${r.code}-${r.date}-${r.balance}`}
      pagination={false}
      dataSource={rows}
      columns={[
        { title: 'Mã', dataIndex: 'code', width: 90 },
        { title: 'Ngày', dataIndex: 'date', width: 100 },
        {
          title: 'Loại',
          dataIndex: 'kind',
          width: 105,
          render: (k: string) => <Tag color={MOVE_KIND_COLORS[k]}>{MOVE_KIND_LABELS[k] ?? k}</Tag>,
        },
        { title: 'Kho', dataIndex: 'warehouse', ellipsis: true },
        {
          title: 'Nhập',
          dataIndex: 'in_qty',
          align: 'right' as const,
          width: 80,
          render: (v: number) => (v ? <Text style={{ color: '#10b981' }}>{fmtQty(v)}</Text> : ''),
        },
        {
          title: 'Xuất',
          dataIndex: 'out_qty',
          align: 'right' as const,
          width: 80,
          render: (v: number) => (v ? <Text style={{ color: '#f5222d' }}>{fmtQty(v)}</Text> : ''),
        },
        { title: 'Tồn', dataIndex: 'balance', align: 'right' as const, width: 90, render: (v: number) => <Text strong>{fmtQty(v)}</Text> },
      ]}
    />
  )
}

export function StockCardTab() {
  const [products, setProducts] = useState<Product[]>([])
  const [warehouses, setWarehouses] = useState<Warehouse[]>([])
  const [productId, setProductId] = useState<number | undefined>()
  const [warehouseId, setWarehouseId] = useState<number | undefined>()
  const [rows, setRows] = useState<CardRow[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    api.products({}).then((r) => setProducts(r.products))
    api.warehouses().then((r) => setWarehouses(r.warehouses))
  }, [])

  useEffect(() => {
    if (!productId) {
      setRows([])
      return
    }
    setLoading(true)
    api
      .stockCard(productId, warehouseId)
      .then((r) => setRows(r.card ?? []))
      .finally(() => setLoading(false))
  }, [productId, warehouseId])

  return (
    <Card
      size="small"
      title={
        <Flex gap={8}>
          <Select
            showSearch
            optionFilterProp="label"
            placeholder="Chọn sản phẩm…"
            style={{ width: 300 }}
            value={productId}
            onChange={setProductId}
            options={products.map((p) => ({ value: p.id, label: `${p.name}${p.sku ? ` (${p.sku})` : ''}` }))}
          />
          <Select
            allowClear
            placeholder="Tất cả kho"
            style={{ width: 180 }}
            value={warehouseId}
            onChange={setWarehouseId}
            options={warehouses.map((w) => ({ value: w.id, label: w.name }))}
          />
        </Flex>
      }
    >
      {loading ? (
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      ) : productId ? (
        <CardTable rows={rows} />
      ) : (
        <Empty description="Chọn một sản phẩm để xem thẻ kho" image={Empty.PRESENTED_IMAGE_SIMPLE} />
      )}
    </Card>
  )
}

// ---- Kho ----

export function WarehousesTab({ onChange }: { onChange: () => void }) {
  const [items, setItems] = useState<Warehouse[]>([])
  const [loading, setLoading] = useState(true)
  const [form] = Form.useForm()

  const load = () => {
    setLoading(true)
    api
      .warehouses()
      .then((r) => setItems(r.warehouses))
      .finally(() => setLoading(false))
  }
  useEffect(load, [])

  const add = async (v: any) => {
    const r = await api.warehouseAdd(v)
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success('Đã thêm kho')
    form.resetFields()
    load()
    onChange()
  }

  const toggle = async (w: Warehouse) => {
    const r = await api.warehouseUpdate(w.id, { status: w.status === 'active' ? 'inactive' : 'active' })
    if (r.error) {
      message.error(r.error)
      return
    }
    load()
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Card size="small" title="Thêm kho / chi nhánh">
        <Form form={form} layout="inline" onFinish={add} style={{ rowGap: 8 }}>
          <Form.Item name="name" rules={[{ required: true, message: 'Nhập tên kho' }]}>
            <Input placeholder="Tên kho" style={{ width: 200 }} />
          </Form.Item>
          <Form.Item name="location">
            <Input placeholder="Địa chỉ" style={{ width: 240 }} />
          </Form.Item>
          <Button type="primary" htmlType="submit" icon={<PlusOutlined />}>
            Thêm
          </Button>
        </Form>
      </Card>
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={false}
        columns={[
          { title: 'ID', dataIndex: 'id', width: 60 },
          {
            title: 'Tên',
            dataIndex: 'name',
            render: (v: string, r: Warehouse) => (
              <Space size={6}>
                {v}
                {r.status === 'inactive' && <Tag>ngừng dùng</Tag>}
              </Space>
            ),
          },
          { title: 'Địa chỉ', dataIndex: 'location', render: (v: string) => v || '—' },
          { title: 'Mặt hàng', dataIndex: 'sku_count', align: 'right' as const, width: 100 },
          { title: 'Giá trị tồn', dataIndex: 'stock_value', align: 'right' as const, width: 140, render: (v: number) => fmtMoney(v) },
          {
            title: '',
            width: 110,
            render: (_: any, r: Warehouse) => (
              <Button size="small" onClick={() => toggle(r)}>
                {r.status === 'active' ? 'Ngừng dùng' : 'Kích hoạt'}
              </Button>
            ),
          },
        ]}
      />
    </Space>
  )
}

// ---- Đối tác ----

export function PartnersTab() {
  const [items, setItems] = useState<Partner[]>([])
  const [loading, setLoading] = useState(true)
  const [form] = Form.useForm()

  const load = () => {
    setLoading(true)
    api
      .partners()
      .then((r) => setItems(r.partners))
      .finally(() => setLoading(false))
  }
  useEffect(load, [])

  const add = async (v: any) => {
    const r = await api.partnerAdd(v)
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success('Đã thêm đối tác')
    form.resetFields()
    load()
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Card size="small" title="Thêm đối tác">
        <Form form={form} layout="inline" onFinish={add} initialValues={{ kind: 'supplier' }} style={{ rowGap: 8 }}>
          <Form.Item name="name" rules={[{ required: true, message: 'Nhập tên' }]}>
            <Input placeholder="Tên đối tác" style={{ width: 200 }} />
          </Form.Item>
          <Form.Item name="kind">
            <Select style={{ width: 150 }} options={Object.entries(PARTNER_KIND_LABELS).map(([value, label]) => ({ value, label }))} />
          </Form.Item>
          <Form.Item name="phone">
            <Input placeholder="SĐT" style={{ width: 130 }} />
          </Form.Item>
          <Form.Item name="address">
            <Input placeholder="Địa chỉ" style={{ width: 200 }} />
          </Form.Item>
          <Button type="primary" htmlType="submit" icon={<PlusOutlined />}>
            Thêm
          </Button>
        </Form>
      </Card>
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={items}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        columns={[
          { title: 'Tên', dataIndex: 'name' },
          {
            title: 'Loại',
            dataIndex: 'kind',
            width: 140,
            render: (k: string) => <Tag color={k === 'supplier' ? 'green' : k === 'customer' ? 'blue' : undefined}>{PARTNER_KIND_LABELS[k] ?? k}</Tag>,
          },
          { title: 'SĐT', dataIndex: 'phone', width: 130, render: (v: string) => v || '—' },
          { title: 'Địa chỉ', dataIndex: 'address', render: (v: string) => v || '—' },
        ]}
      />
    </Space>
  )
}

// ---- Hoạt động ----

export function ActivityTab() {
  const [items, setItems] = useState<any[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    api
      .activity()
      .then((r) => setItems(r.activity))
      .finally(() => setLoading(false))
  }, [])

  return (
    <Card size="small">
      {loading ? (
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      ) : (
        <List
          size="small"
          dataSource={items}
          locale={{ emptyText: <Empty description="Chưa có hoạt động" image={Empty.PRESENTED_IMAGE_SIMPLE} /> }}
          renderItem={(a) => (
            <List.Item>
              <Space>
                <Tag>{a.kind}</Tag>
                <Text>{a.text}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {new Date(a.created_at * 1000).toLocaleString('vi-VN')}
                </Text>
              </Space>
            </List.Item>
          )}
        />
      )}
    </Card>
  )
}
