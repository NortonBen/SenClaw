import { useEffect, useMemo, useState } from 'react'
import {
  Button,
  Card,
  Col,
  Empty,
  Flex,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Table,
  Tag,
  Typography,
  message,
} from 'antd'
import { DeleteOutlined, MinusOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { api, fmtDate, fmtMoney, fmtQty, type MenuItem, type Sale } from './api'

const { Text } = Typography

interface CartLine {
  menu: MenuItem
  qty: number
  price: number
}

export function SalesTab({ onChange }: { onChange: () => void }) {
  const [menu, setMenu] = useState<MenuItem[]>([])
  const [cart, setCart] = useState<CartLine[]>([])
  const [note, setNote] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [reloadKey, setReloadKey] = useState(0)

  useEffect(() => {
    api.menu().then((r) => setMenu(r.menu))
  }, [])

  const categories = useMemo(() => {
    const set = new Set(menu.map((m) => m.category || '(khác)'))
    return Array.from(set)
  }, [menu])

  const add = (m: MenuItem) => {
    setCart((c) => {
      const i = c.findIndex((l) => l.menu.id === m.id)
      if (i >= 0) {
        const next = [...c]
        next[i] = { ...next[i], qty: next[i].qty + 1 }
        return next
      }
      return [...c, { menu: m, qty: 1, price: m.price }]
    })
  }

  const setQty = (id: number, qty: number) => {
    setCart((c) => (qty <= 0 ? c.filter((l) => l.menu.id !== id) : c.map((l) => (l.menu.id === id ? { ...l, qty } : l))))
  }

  const total = cart.reduce((s, l) => s + l.qty * l.price, 0)

  const submit = async () => {
    if (!cart.length) return
    setSubmitting(true)
    try {
      const r = await api.saleCreate({
        note,
        lines: cart.map((l) => ({
          menu_id: l.menu.id,
          qty: l.qty,
          unit_price: l.price !== l.menu.price ? l.price : undefined,
        })),
      })
      if (r.error) {
        message.error(r.error)
        return
      }
      const warnings = r.sale?.warnings ?? []
      if (warnings.length) {
        Modal.warning({
          title: `Đã ghi đơn ${r.sale?.code} — có cảnh báo`,
          content: (
            <ul style={{ paddingLeft: 18 }}>
              {warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          ),
        })
      } else {
        message.success(`Đã ghi đơn ${r.sale?.code} — ${fmtMoney(r.sale?.total ?? 0)}`)
      }
      setCart([])
      setNote('')
      setReloadKey((k) => k + 1)
      onChange()
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Flex vertical gap={12}>
      <Row gutter={[12, 12]}>
        <Col xs={24} lg={15}>
          <Card size="small" title="Chọn món">
            {menu.length === 0 && <Empty description="Thực đơn trống — thêm món ở tab Thực đơn" />}
            <Flex vertical gap={10}>
              {categories.map((cat) => (
                <div key={cat}>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {cat}
                  </Text>
                  <Flex wrap gap={8} style={{ marginTop: 4 }}>
                    {menu
                      .filter((m) => (m.category || '(khác)') === cat)
                      .map((m) => (
                        <Button key={m.id} onClick={() => add(m)} style={{ height: 'auto', padding: '6px 12px' }}>
                          <Flex vertical align="start" gap={0}>
                            <span>{m.name}</span>
                            <Text type="secondary" style={{ fontSize: 11 }}>
                              {fmtMoney(m.price)}
                            </Text>
                          </Flex>
                        </Button>
                      ))}
                  </Flex>
                </div>
              ))}
            </Flex>
          </Card>
        </Col>
        <Col xs={24} lg={9}>
          <Card size="small" title={`Đơn hiện tại (${cart.length} món)`}>
            {cart.length === 0 && <Empty description="Bấm món bên trái để thêm" image={Empty.PRESENTED_IMAGE_SIMPLE} />}
            <Flex vertical gap={8}>
              {cart.map((l) => (
                <Flex key={l.menu.id} align="center" gap={8}>
                  <Text style={{ flex: 1 }} ellipsis>
                    {l.menu.name}
                  </Text>
                  <Button size="small" icon={<MinusOutlined />} onClick={() => setQty(l.menu.id, l.qty - 1)} />
                  <Text style={{ width: 22, textAlign: 'center' }}>{l.qty}</Text>
                  <Button size="small" icon={<PlusOutlined />} onClick={() => setQty(l.menu.id, l.qty + 1)} />
                  <InputNumber
                    size="small"
                    style={{ width: 100 }}
                    min={0}
                    step={1000}
                    value={l.price}
                    onChange={(v) =>
                      setCart((c) => c.map((x) => (x.menu.id === l.menu.id ? { ...x, price: Number(v ?? 0) } : x)))
                    }
                  />
                  <Button size="small" danger icon={<DeleteOutlined />} onClick={() => setQty(l.menu.id, 0)} />
                </Flex>
              ))}
              {cart.length > 0 && (
                <>
                  <Input placeholder="Ghi chú đơn…" value={note} onChange={(e) => setNote(e.target.value)} />
                  <Flex justify="space-between" align="center">
                    <Text strong>Tổng: {fmtMoney(total)}</Text>
                    <Button type="primary" loading={submitting} onClick={submit}>
                      Thanh toán (ghi đơn)
                    </Button>
                  </Flex>
                </>
              )}
            </Flex>
          </Card>
        </Col>
      </Row>
      <RecentSales reloadKey={reloadKey} onChange={onChange} />
    </Flex>
  )
}

function RecentSales({ reloadKey, onChange }: { reloadKey: number; onChange: () => void }) {
  const [sales, setSales] = useState<Sale[]>([])
  const [loading, setLoading] = useState(true)
  const [detail, setDetail] = useState<Sale | null>(null)

  const load = () => {
    setLoading(true)
    api
      .sales({ limit: 30 })
      .then((r) => setSales(r.sales))
      .finally(() => setLoading(false))
  }
  useEffect(load, [reloadKey])

  const doVoid = async (id: number) => {
    const r = await api.saleVoid(id, 'huỷ từ giao diện')
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success('Đã huỷ đơn và hoàn nguyên liệu về kho')
    load()
    onChange()
  }

  const openDetail = async (id: number) => {
    const r = await api.saleGet(id)
    if (r.sale) setDetail(r.sale)
  }

  return (
    <Card
      size="small"
      title="Đơn gần đây"
      extra={<Button size="small" icon={<ReloadOutlined />} onClick={load} />}
    >
      <Table
        size="small"
        rowKey="id"
        loading={loading}
        dataSource={sales}
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        onRow={(r) => ({ onClick: () => openDetail(r.id), style: { cursor: 'pointer' } })}
        columns={[
          { title: 'Mã', dataIndex: 'code', render: (c, r) => <Text delete={r.status === 'void'}>{c}</Text> },
          { title: 'Ngày', dataIndex: 'sale_date', render: fmtDate },
          { title: 'Món', dataIndex: 'items', ellipsis: true },
          { title: 'Tổng', dataIndex: 'total', align: 'right', render: fmtMoney },
          { title: 'Lãi gộp', dataIndex: 'profit', align: 'right', render: fmtMoney },
          {
            title: 'TT',
            dataIndex: 'status',
            render: (s) => (s === 'void' ? <Tag>đã huỷ</Tag> : <Tag color="green">done</Tag>),
          },
          {
            title: '',
            key: 'actions',
            render: (_, r) =>
              r.status === 'done' && (
                <Popconfirm
                  title="Huỷ đơn này? Nguyên liệu sẽ hoàn về kho."
                  onConfirm={(e) => {
                    e?.stopPropagation()
                    doVoid(r.id)
                  }}
                  onCancel={(e) => e?.stopPropagation()}
                >
                  <Button size="small" danger onClick={(e) => e.stopPropagation()}>
                    Huỷ
                  </Button>
                </Popconfirm>
              ),
          },
        ]}
      />
      <Modal open={!!detail} onCancel={() => setDetail(null)} footer={null} title={detail?.code} width={560}>
        {detail && (
          <Flex vertical gap={8}>
            <Text type="secondary">
              Ngày {fmtDate(detail.sale_date)} · {detail.status === 'void' ? 'ĐÃ HUỶ' : 'hoàn tất'}
              {detail.note ? ` · ${detail.note}` : ''}
            </Text>
            <Table
              size="small"
              rowKey="id"
              dataSource={detail.lines ?? []}
              pagination={false}
              columns={[
                { title: 'Món', dataIndex: 'menu_name' },
                { title: 'SL', dataIndex: 'qty', align: 'right', render: fmtQty },
                { title: 'Đơn giá', dataIndex: 'unit_price', align: 'right', render: fmtMoney },
                { title: 'Thành tiền', dataIndex: 'amount', align: 'right', render: fmtMoney },
                { title: 'Giá vốn', dataIndex: 'cogs', align: 'right', render: fmtMoney },
              ]}
            />
            <Flex justify="end" gap={16}>
              <Text>
                Tổng: <Text strong>{fmtMoney(detail.total)}</Text>
              </Text>
              <Text>
                Lãi gộp: <Text strong>{fmtMoney(detail.profit)}</Text>
              </Text>
            </Flex>
          </Flex>
        )}
      </Modal>
    </Card>
  )
}
