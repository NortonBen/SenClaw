import { useEffect, useState } from 'react'
import {
  Alert,
  Badge,
  Button,
  Card,
  Empty,
  Flex,
  Form,
  Input,
  List,
  Segmented,
  Space,
  Tabs,
  Tag,
  Typography,
  message,
} from 'antd'
import { api, type Draft, type SettingsPublic, type Status } from './api'

const { Title, Text, Paragraph } = Typography

export default function App() {
  const [status, setStatus] = useState<Status | null>(null)

  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 15000)
    return () => clearInterval(t)
  }, [])

  return (
    <div style={{ maxWidth: 960, margin: '0 auto', padding: 24 }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 8 }}>
        <Title level={3} style={{ margin: 0 }}>
          🛒 Shopee <Text type="secondary" style={{ fontSize: 14 }}>— SenClaw</Text>
        </Title>
        <Space>
          {status?.connected ? (
            <Badge status="success" text="Đã kết nối shop" />
          ) : (
            <Badge status="default" text="Chưa kết nối" />
          )}
          <Tag color="orange">autonomy: {status?.autonomy ?? '—'}</Tag>
        </Space>
      </Flex>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Chỉ dùng Shopee Open Platform chính thức. Mọi câu trả lời khách là draft-first — chỉ gửi khi bạn Duyệt."
      />

      <Tabs
        defaultActiveKey="connect"
        items={[
          { key: 'connect', label: 'Kết nối', children: <ConnectTab onChange={refreshStatus} /> },
          { key: 'orders', label: 'Đơn hàng', children: <OrdersTab /> },
          { key: 'products', label: 'Sản phẩm', children: <ProductsTab /> },
          { key: 'chat', label: 'Hội thoại', children: <ChatTab onChange={refreshStatus} /> },
          {
            key: 'drafts',
            label: <Badge count={status?.pending_drafts ?? 0} size="small" offset={[8, -2]}>Hàng chờ duyệt</Badge>,
            children: <DraftsTab onChange={refreshStatus} />,
          },
          { key: 'activity', label: 'Hoạt động', children: <ActivityTab /> },
        ]}
      />
    </div>
  )
}

function ConnectTab({ onChange }: { onChange: () => void }) {
  const [settings, setSettings] = useState<SettingsPublic | null>(null)
  const [form] = Form.useForm()

  const load = () =>
    api.getSettings().then((s) => {
      setSettings(s)
      form.setFieldsValue({ partner_id: s.partner_id, shop_id: s.shop_id, host: s.host })
    })
  useEffect(() => {
    load()
  }, [])

  const save = async (v: any) => {
    await api.setSettings(v)
    message.success('Đã lưu cấu hình')
    await load()
    onChange()
  }

  const authorize = async () => {
    const redirect = `${location.origin}/api/oauth/callback`
    const r = await api.oauthLink(redirect)
    if (r.url) {
      window.open(r.url, '_blank')
      message.info('Đã mở link cấp quyền — seller bấm Đồng ý (link sống 5 phút)')
    } else {
      message.error(r.error || 'Chưa cấu hình partner_id/partner_key')
    }
  }

  const setAutonomy = async (val: string) => {
    await api.setSettings({ autonomy: val })
    onChange()
    load()
  }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="1. Partner credentials (từ open.shopee.com)" size="small">
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Bạn đăng ký Partner App trên open.shopee.com để lấy <Text code>partner_id</Text> +{' '}
          <Text code>partner_key</Text>. <Text code>partner_key</Text> chỉ lưu cục bộ, không hiển thị lại.
        </Paragraph>
        <Form form={form} layout="vertical" onFinish={save}>
          <Flex gap={12} wrap>
            <Form.Item name="partner_id" label="partner_id" style={{ flex: 1, minWidth: 160 }}>
              <Input placeholder="200xxxx" />
            </Form.Item>
            <Form.Item
              name="partner_key"
              label={settings?.partner_key_set ? 'partner_key (đã đặt — nhập lại để đổi)' : 'partner_key'}
              style={{ flex: 1, minWidth: 220 }}
            >
              <Input.Password placeholder={settings?.partner_key_set ? '••••••••' : 'shpk_...'} />
            </Form.Item>
          </Flex>
          <Flex gap={12} wrap>
            <Form.Item name="shop_id" label="shop_id (tự điền sau khi authorize)" style={{ flex: 1, minWidth: 160 }}>
              <Input placeholder="55xxxxxx" />
            </Form.Item>
            <Form.Item name="host" label="host (để trống = live)" style={{ flex: 1, minWidth: 260 }}>
              <Input placeholder="https://partner.shopeemobile.com" />
            </Form.Item>
          </Flex>
          <Button type="primary" htmlType="submit">Lưu</Button>
        </Form>
      </Card>

      <Card title="2. Authorize shop (OAuth)" size="small">
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Mở link để seller tự bấm Đồng ý. Shopee sẽ redirect về{' '}
          <Text code>/api/oauth/callback</Text> kèm <Text code>code</Text> + <Text code>shop_id</Text>, app tự đổi token.
        </Paragraph>
        <Button onClick={authorize}>Mở link cấp quyền</Button>
      </Card>

      <Card title="3. Chế độ tự động (autonomy)" size="small">
        <Segmented
          value={settings?.autonomy ?? 'draft'}
          onChange={(v) => setAutonomy(String(v))}
          options={[
            { label: 'Observe (chỉ đọc)', value: 'observe' },
            { label: 'Draft (soạn, chờ duyệt)', value: 'draft' },
            { label: 'Live (tự gửi)', value: 'live' },
          ]}
        />
        <Paragraph type="secondary" style={{ marginBottom: 0, marginTop: 8 }}>
          <b>Draft</b> (khuyến nghị): heartbeat soạn sẵn trả lời cho tin chưa đọc, bạn duyệt mới gửi.
        </Paragraph>
      </Card>
    </Space>
  )
}

function RawJson({ load }: { load: () => Promise<any> }) {
  const [data, setData] = useState<any>(null)
  const [loading, setLoading] = useState(false)
  const run = () => {
    setLoading(true)
    load().then(setData).finally(() => setLoading(false))
  }
  useEffect(() => {
    run()
  }, [])
  if (data?.error) return <Alert type="warning" showIcon message={String(data.error)} action={<Button size="small" onClick={run}>Thử lại</Button>} />
  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      <Button size="small" loading={loading} onClick={run}>Làm mới</Button>
      <pre style={{ background: '#1f1f1f', padding: 12, borderRadius: 8, overflow: 'auto', maxHeight: 480 }}>
        {data ? JSON.stringify(data, null, 2) : '…'}
      </pre>
    </Space>
  )
}

function OrdersTab() {
  return <RawJson load={api.orders} />
}

function ProductsTab() {
  const [form] = Form.useForm()
  const doStock = async (v: any) => {
    const r = await api.updateStock(Number(v.item_id), Number(v.stock))
    if (r.error) message.error(String(r.error))
    else message.success('Đã cập nhật tồn kho')
  }
  const doPrice = async (v: any) => {
    const r = await api.updatePrice(Number(v.item_id), Number(v.price))
    if (r.error) message.error(String(r.error))
    else message.success('Đã cập nhật giá')
  }
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="Sản phẩm của shop (status=NORMAL)" size="small">
        <RawJson load={() => api.products('NORMAL')} />
      </Card>
      <Card title="Cập nhật tồn kho / giá (variant đơn)" size="small">
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Ghi lên shop của <b>chính bạn</b> qua Product API. Không tự động hoá — chỉ chạy khi bạn bấm.
        </Paragraph>
        <Form form={form} layout="inline" style={{ rowGap: 12 }}>
          <Form.Item name="item_id" label="item_id" rules={[{ required: true }]}>
            <Input placeholder="123456" style={{ width: 140 }} />
          </Form.Item>
          <Form.Item name="stock" label="tồn kho">
            <Input placeholder="100" style={{ width: 100 }} />
          </Form.Item>
          <Button onClick={() => form.validateFields(['item_id', 'stock']).then(doStock)}>Cập nhật tồn</Button>
          <Form.Item name="price" label="giá">
            <Input placeholder="199000" style={{ width: 120 }} />
          </Form.Item>
          <Button onClick={() => form.validateFields(['item_id', 'price']).then(doPrice)}>Cập nhật giá</Button>
        </Form>
      </Card>
    </Space>
  )
}

function ChatTab({ onChange }: { onChange: () => void }) {
  const [form] = Form.useForm()
  const submit = async (v: any) => {
    const r = await api.reply({
      conversation_id: v.conversation_id,
      to_id: Number(v.to_id),
      to_name: v.to_name,
      content: v.content || undefined,
      customer_msg: v.customer_msg,
      context: v.context,
      order_sn: v.order_sn || undefined,
    })
    if (r.error) message.error(String(r.error))
    else message.success(r.status === 'sent' ? 'Đã gửi (live)' : `Đã tạo nháp #${r.draft_id}`)
    onChange()
  }
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="Hội thoại buyer↔seller" size="small">
        <RawJson load={api.conversations} />
      </Card>
      <Card title="Soạn trả lời (draft-first)" size="small">
        <Form form={form} layout="vertical" onFinish={submit}>
          <Flex gap={12} wrap>
            <Form.Item name="conversation_id" label="conversation_id" rules={[{ required: true }]} style={{ flex: 1, minWidth: 180 }}>
              <Input />
            </Form.Item>
            <Form.Item name="to_id" label="to_id (khách)" rules={[{ required: true }]} style={{ width: 160 }}>
              <Input />
            </Form.Item>
            <Form.Item name="to_name" label="tên khách" style={{ width: 160 }}>
              <Input />
            </Form.Item>
          </Flex>
          <Form.Item name="customer_msg" label="Tin của khách (để AI soạn)">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="order_sn" label="order_sn (tra đơn thật để ground câu trả lời)">
            <Input placeholder="2506XXXXXXXXXX" />
          </Form.Item>
          <Form.Item name="context" label="Bối cảnh thêm — chính sách… (tuỳ chọn)">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="content" label="Nội dung tự viết (bỏ trống = AI soạn)">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Button type="primary" htmlType="submit">Tạo nháp</Button>
        </Form>
      </Card>
    </Space>
  )
}

function DraftsTab({ onChange }: { onChange: () => void }) {
  const [drafts, setDrafts] = useState<Draft[]>([])
  const load = () => api.drafts().then((d) => setDrafts(d.pending)).catch(() => {})
  useEffect(() => {
    load()
  }, [])

  const approve = async (id: number) => {
    const r = await api.approve(id)
    if (r.error) message.error(String(r.error))
    else message.success('Đã gửi cho khách')
    load()
    onChange()
  }
  const reject = async (id: number) => {
    await api.reject(id)
    message.info('Đã bỏ nháp')
    load()
    onChange()
  }

  if (!drafts.length) return <Empty description="Không có bản nháp chờ duyệt" />
  return (
    <List
      dataSource={drafts}
      renderItem={(d) => (
        <List.Item
          actions={[
            <Button key="a" type="primary" size="small" onClick={() => approve(d.id)}>Duyệt & gửi</Button>,
            <Button key="r" size="small" danger onClick={() => reject(d.id)}>Bỏ</Button>,
          ]}
        >
          <List.Item.Meta
            title={<Space><Text strong>{d.to_name || `khách ${d.to_id}`}</Text><Tag>{d.source}</Tag>{d.model && <Tag color="blue">{d.model}</Tag>}</Space>}
            description={<Paragraph style={{ marginBottom: 0 }}>{d.content}</Paragraph>}
          />
        </List.Item>
      )}
    />
  )
}

function ActivityTab() {
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
            {a.ref && <Text type="secondary">({a.ref})</Text>}
          </Space>
        </List.Item>
      )}
    />
  )
}
