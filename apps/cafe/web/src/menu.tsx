import { useEffect, useMemo, useState } from 'react'
import {
  AutoComplete,
  Button,
  Card,
  Checkbox,
  Drawer,
  Flex,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Switch,
  Table,
  Tag,
  Typography,
  message,
} from 'antd'
import { PlusOutlined, ReloadOutlined, RobotOutlined } from '@ant-design/icons'
import { api, fmtMoney, fmtQty, type Ingredient, type MenuItem } from './api'

const { Paragraph, Text } = Typography

export function MenuTab({ onChange }: { onChange: () => void }) {
  const [items, setItems] = useState<MenuItem[]>([])
  const [ingredients, setIngredients] = useState<Ingredient[]>([])
  const [loading, setLoading] = useState(true)
  const [q, setQ] = useState('')
  const [showInactive, setShowInactive] = useState(false)
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<MenuItem | null>(null)
  const [recipeOf, setRecipeOf] = useState<MenuItem | null>(null)

  const load = () => {
    setLoading(true)
    api
      .menu({ q: q || undefined, include_inactive: showInactive })
      .then((r) => setItems(r.menu))
      .finally(() => setLoading(false))
    api.ingredients().then((r) => setIngredients(r.ingredients))
  }
  useEffect(load, [q, showInactive])

  const categories = useMemo(
    () => Array.from(new Set(items.map((m) => m.category).filter(Boolean))),
    [items],
  )

  const openRecipe = async (m: MenuItem) => {
    const r = await api.menuGet(m.id)
    if (r.menu) setRecipeOf(r.menu)
  }

  return (
    <Flex vertical gap={12}>
      <Card
        size="small"
        title={
          <Flex gap={8} align="center">
            <Input.Search allowClear placeholder="Tìm món (không dấu cũng được)…" style={{ width: 240 }} onSearch={setQ} />
            <Checkbox checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)}>
              Hiện món ngừng bán
            </Checkbox>
          </Flex>
        }
        extra={
          <Flex gap={8}>
            <Button size="small" icon={<ReloadOutlined />} onClick={load} />
            <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
              Thêm món
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
          onRow={(r) => ({ onClick: () => openRecipe(r), style: { cursor: 'pointer' } })}
          columns={[
            {
              title: 'Món',
              dataIndex: 'name',
              render: (n, r) => (
                <Flex vertical gap={0}>
                  <Text delete={r.status === 'inactive'}>{n}</Text>
                  {r.category && (
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      {r.category}
                    </Text>
                  )}
                </Flex>
              ),
            },
            { title: 'Giá bán', dataIndex: 'price', align: 'right', render: fmtMoney },
            { title: 'Giá vốn', dataIndex: 'cost', align: 'right', render: fmtMoney },
            {
              title: 'Lãi gộp',
              dataIndex: 'margin',
              align: 'right',
              render: (v, r) => (
                <Flex vertical gap={0} align="end">
                  <Text style={{ color: v >= 0 ? '#10b981' : '#f5222d' }}>{fmtMoney(v)}</Text>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {r.margin_pct}%
                  </Text>
                </Flex>
              ),
            },
            {
              title: 'Công thức',
              dataIndex: 'has_recipe',
              render: (h) => (h ? <Tag color="green">có</Tag> : <Tag color="red">chưa có</Tag>),
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
                      openRecipe(r)
                    }}
                  >
                    Công thức
                  </Button>
                </Flex>
              ),
            },
          ]}
        />
      </Card>

      <MenuSuggestCard />

      <MenuFormModal
        open={adding || !!editing}
        item={editing}
        categories={categories}
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

      <RecipeDrawer
        menu={recipeOf}
        ingredients={ingredients}
        onClose={() => setRecipeOf(null)}
        onSaved={(m) => {
          setRecipeOf(m)
          load()
          onChange()
        }}
      />
    </Flex>
  )
}

function MenuFormModal({
  open,
  item,
  categories,
  onClose,
  onSaved,
}: {
  open: boolean
  item: MenuItem | null
  categories: string[]
  onClose: () => void
  onSaved: () => void
}) {
  const [form] = Form.useForm()

  useEffect(() => {
    if (open) {
      form.setFieldsValue(
        item
          ? { name: item.name, category: item.category, price: item.price, instructions: item.instructions, active: item.status === 'active' }
          : { name: '', category: '', price: undefined, instructions: '', active: true },
      )
    }
  }, [open, item, form])

  const submit = async () => {
    const v = await form.validateFields()
    const r = item
      ? await api.menuUpdate(item.id, v)
      : await api.menuAdd({ name: v.name, category: v.category ?? '', price: v.price ?? 0, instructions: v.instructions ?? '' })
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success(item ? 'Đã cập nhật món' : 'Đã thêm món — nhớ đặt công thức để tính giá vốn')
    onSaved()
  }

  return (
    <Modal open={open} onCancel={onClose} onOk={submit} title={item ? `Sửa: ${item.name}` : 'Thêm món'} okText="Lưu">
      <Form form={form} layout="vertical">
        <Form.Item name="name" label="Tên món" rules={[{ required: true, message: 'Nhập tên món' }]}>
          <Input placeholder="Cafe sữa đá…" />
        </Form.Item>
        <Form.Item name="category" label="Nhóm">
          <AutoComplete options={categories.map((c) => ({ value: c }))} placeholder="Cà phê / Trà / Sinh tố…" />
        </Form.Item>
        <Form.Item name="price" label="Giá bán (đ)" rules={[{ required: true, message: 'Nhập giá bán' }]}>
          <InputNumber style={{ width: '100%' }} min={0} step={1000} />
        </Form.Item>
        <Form.Item name="instructions" label="Cách pha chế">
          <Input.TextArea rows={3} placeholder="Các bước pha chế…" />
        </Form.Item>
        {item && (
          <Form.Item name="active" label="Đang bán" valuePropName="checked">
            <Switch />
          </Form.Item>
        )}
      </Form>
    </Modal>
  )
}

function RecipeDrawer({
  menu,
  ingredients,
  onClose,
  onSaved,
}: {
  menu: MenuItem | null
  ingredients: Ingredient[]
  onClose: () => void
  onSaved: (m: MenuItem) => void
}) {
  const [items, setItems] = useState<{ ingredient_id: number; qty: number }[]>([])
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    setItems((menu?.recipe ?? []).map((r) => ({ ingredient_id: r.ingredient_id, qty: r.qty })))
  }, [menu])

  const ingOf = (id: number) => ingredients.find((i) => i.id === id)
  const cost = items.reduce((s, it) => s + it.qty * (ingOf(it.ingredient_id)?.avg_cost ?? 0), 0)
  const available = ingredients.filter((i) => !items.some((it) => it.ingredient_id === i.id))

  const save = async () => {
    if (!menu) return
    setSaving(true)
    try {
      const r = await api.recipeSet(menu.id, items)
      if (r.error) {
        message.error(r.error)
        return
      }
      message.success('Đã lưu công thức')
      if (r.menu) onSaved(r.menu)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Drawer open={!!menu} onClose={onClose} width={520} title={menu ? `Công thức: ${menu.name}` : ''}>
      {menu && (
        <Flex vertical gap={12}>
          {menu.instructions && (
            <Card size="small" title="Cách pha chế">
              <Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 0 }}>{menu.instructions}</Paragraph>
            </Card>
          )}
          <Table
            size="small"
            rowKey="ingredient_id"
            dataSource={items}
            pagination={false}
            locale={{ emptyText: 'Chưa có nguyên liệu — thêm bên dưới' }}
            columns={[
              { title: 'Nguyên liệu', dataIndex: 'ingredient_id', render: (id) => ingOf(id)?.name ?? `#${id}` },
              {
                title: 'Định lượng',
                dataIndex: 'qty',
                width: 140,
                render: (v, r) => (
                  <InputNumber
                    size="small"
                    min={0.001}
                    value={v}
                    addonAfter={ingOf(r.ingredient_id)?.unit}
                    onChange={(nv) =>
                      setItems((xs) => xs.map((x) => (x.ingredient_id === r.ingredient_id ? { ...x, qty: Number(nv ?? 0) } : x)))
                    }
                  />
                ),
              },
              {
                title: 'Giá vốn dòng',
                key: 'cost',
                align: 'right',
                render: (_, r) => fmtMoney(r.qty * (ingOf(r.ingredient_id)?.avg_cost ?? 0)),
              },
              {
                title: '',
                key: 'rm',
                width: 60,
                render: (_, r) => (
                  <Button size="small" danger onClick={() => setItems((xs) => xs.filter((x) => x.ingredient_id !== r.ingredient_id))}>
                    Xoá
                  </Button>
                ),
              },
            ]}
          />
          <Select
            showSearch
            placeholder="+ Thêm nguyên liệu vào công thức…"
            value={null}
            optionFilterProp="label"
            options={available.map((i) => ({
              value: i.id,
              label: `${i.name} (${i.unit} · ${fmtMoney(i.avg_cost)}/${i.unit})`,
            }))}
            onSelect={(id) => setItems((xs) => [...xs, { ingredient_id: Number(id), qty: 1 }])}
          />
          <Flex justify="space-between" align="center">
            <Text>
              Giá vốn ước tính: <Text strong>{fmtMoney(cost)}</Text>
              {menu.price > 0 && (
                <Text type="secondary"> · lãi {fmtMoney(menu.price - cost)} ({fmtQty(menu.price > 0 ? ((menu.price - cost) / menu.price) * 100 : 0)}%)</Text>
              )}
            </Text>
            <Button type="primary" loading={saving} onClick={save}>
              Lưu công thức
            </Button>
          </Flex>
        </Flex>
      )}
    </Drawer>
  )
}

function MenuSuggestCard() {
  const [idea, setIdea] = useState('')
  const [margin, setMargin] = useState<number>(70)
  const [loading, setLoading] = useState(false)
  const [result, setResult] = useState<{ suggestion: string; model: string } | null>(null)

  const run = async () => {
    setLoading(true)
    try {
      setResult(await api.menuSuggest(idea, margin))
    } finally {
      setLoading(false)
    }
  }

  return (
    <Card size="small" title={<><RobotOutlined /> AI gợi ý món mới từ nguyên liệu sẵn có</>}>
      <Flex gap={8} wrap>
        <Input
          style={{ flex: 1, minWidth: 240 }}
          placeholder="Ý tưởng (vd: một món trà trái cây mùa hè)… bỏ trống = AI tự gợi ý"
          value={idea}
          onChange={(e) => setIdea(e.target.value)}
          onPressEnter={run}
        />
        <InputNumber min={0} max={95} value={margin} addonAfter="% lãi mục tiêu" onChange={(v) => setMargin(Number(v ?? 70))} />
        <Button type="primary" loading={loading} onClick={run}>
          Gợi ý
        </Button>
      </Flex>
      {result && (
        <div style={{ marginTop: 12 }}>
          <Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 4 }}>{result.suggestion}</Paragraph>
          {result.model && <Tag>{result.model}</Tag>}
        </div>
      )}
    </Card>
  )
}
