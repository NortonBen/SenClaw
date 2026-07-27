import { useEffect, useState, type ReactNode } from 'react'
import {
  Alert, App as AntdApp, Badge, Button, Card, ConfigProvider, Empty, Flex, Form, Input, Layout,
  List, Menu, Segmented, Select, Space, Steps, Switch, Table, Tag, theme, Tooltip, Typography, Upload, message,
} from 'antd'
import {
  ApiOutlined, BarChartOutlined, BulbFilled, BulbOutlined, DashboardOutlined, EditOutlined,
  FileTextOutlined, FundOutlined, HistoryOutlined, InboxOutlined, LinkOutlined, MenuFoldOutlined,
  MenuUnfoldOutlined, MessageOutlined, ProfileOutlined, ThunderboltOutlined, UploadOutlined,
} from '@ant-design/icons'
import { api, openExternal, type Draft, type Page, type SettingsPublic, type Status, type Trigger } from './api'

const { Text, Paragraph } = Typography
const { Sider, Header, Content } = Layout

const NAV: { key: string; label: string; icon: ReactNode }[] = [
  { key: 'connect', label: 'Kết nối & Cài đặt', icon: <ApiOutlined /> },
  { key: 'overview', label: 'Tổng quan', icon: <DashboardOutlined /> },
  { key: 'pages', label: 'Trang', icon: <ProfileOutlined /> },
  { key: 'compose', label: 'Đăng bài', icon: <EditOutlined /> },
  { key: 'posts', label: 'Bài & bình luận', icon: <FileTextOutlined /> },
  { key: 'inbox', label: 'Tin nhắn', icon: <MessageOutlined /> },
  { key: 'analytics', label: 'Thống kê & phân tích', icon: <BarChartOutlined /> },
  { key: 'ads', label: 'Quảng cáo (Ads)', icon: <FundOutlined /> },
  { key: 'triggers', label: 'Trigger', icon: <ThunderboltOutlined /> },
  { key: 'drafts', label: 'Hàng chờ duyệt', icon: <InboxOutlined /> },
  { key: 'activity', label: 'Hoạt động', icon: <HistoryOutlined /> },
]

export default function App() {
  const [mode, setMode] = useState<'dark' | 'light'>(
    () => (localStorage.getItem('fbpro-theme') as 'dark' | 'light') || 'dark',
  )
  useEffect(() => {
    localStorage.setItem('fbpro-theme', mode)
    document.body.style.background = mode === 'dark' ? '#000000' : '#f0f2f5'
  }, [mode])

  return (
    <ConfigProvider
      theme={{
        algorithm: mode === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#1877f2', borderRadius: 10 },
      }}
    >
      <AntdApp>
        <Shell mode={mode} setMode={setMode} />
      </AntdApp>
    </ConfigProvider>
  )
}

function Shell({ mode, setMode }: { mode: 'dark' | 'light'; setMode: (m: 'dark' | 'light') => void }) {
  const [status, setStatus] = useState<Status | null>(null)
  const [active, setActive] = useState('connect')
  const [collapsed, setCollapsed] = useState(false)
  const { token } = theme.useToken()
  const refreshStatus = () => api.status().then(setStatus).catch(() => {})
  useEffect(() => {
    refreshStatus()
    const t = setInterval(refreshStatus, 15000)
    return () => clearInterval(t)
  }, [])

  const sections: Record<string, ReactNode> = {
    connect: <ConnectTab onChange={refreshStatus} />,
    overview: <OverviewTab />,
    pages: <PagesTab active={status?.active_page_id} onChange={refreshStatus} />,
    compose: <ComposeTab onChange={refreshStatus} />,
    posts: <PostsTab onChange={refreshStatus} />,
    inbox: <InboxTab onChange={refreshStatus} />,
    analytics: <AnalyticsTab />,
    ads: <AdsTab />,
    triggers: <TriggersTab />,
    drafts: <DraftsTab onChange={refreshStatus} />,
    activity: <ActivityTab />,
  }

  const menuItems = NAV.map((n) => ({
    key: n.key,
    icon: n.icon,
    label:
      n.key === 'drafts' && (status?.pending_drafts ?? 0) > 0 ? (
        <Badge count={status?.pending_drafts} size="small" offset={[12, 0]}>
          {n.label}
        </Badge>
      ) : (
        n.label
      ),
  }))
  const title = NAV.find((n) => n.key === active)?.label ?? ''

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider
        theme={mode}
        collapsible
        collapsed={collapsed}
        onCollapse={setCollapsed}
        trigger={null}
        breakpoint="lg"
        collapsedWidth={64}
        width={224}
      >
        <div
          style={{
            height: 56, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8,
            fontSize: collapsed ? 22 : 18, fontWeight: 700, color: token.colorText,
          }}
        >
          📘{!collapsed && <span>Facebook Pro</span>}
        </div>
        <Menu theme={mode} mode="inline" selectedKeys={[active]} items={menuItems} onClick={(e) => setActive(e.key)} />
      </Sider>
      <Layout>
        <Header
          style={{
            background: token.colorBgContainer, borderBottom: `1px solid ${token.colorSplit}`,
            padding: '0 16px', height: 56, lineHeight: '56px',
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          }}
        >
          <Space>
            <Button type="text" icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />} onClick={() => setCollapsed(!collapsed)} />
            <Text strong style={{ fontSize: 16 }}>{title}</Text>
          </Space>
          <Space size="middle">
            {status?.connected ? <Badge status="success" text="Đã kết nối" /> : <Badge status="default" text="Chưa kết nối" />}
            <Tag color="blue">autonomy: {status?.autonomy ?? '—'}</Tag>
            <Tooltip title={mode === 'dark' ? 'Chuyển giao diện sáng' : 'Chuyển giao diện tối'}>
              <Switch
                checked={mode === 'dark'}
                onChange={(v) => setMode(v ? 'dark' : 'light')}
                checkedChildren={<BulbFilled />}
                unCheckedChildren={<BulbOutlined />}
              />
            </Tooltip>
          </Space>
        </Header>
        <Content style={{ padding: 24, overflow: 'auto' }}>
          <div style={{ maxWidth: 1040, margin: '0 auto' }}>
            {active === 'connect' && (
              <Alert
                type="info"
                showIcon
                style={{ marginBottom: 16 }}
                message="Chỉ dùng Facebook Graph API chính thức qua Developer App của bạn. Mọi bài & trả lời là draft-first — chỉ đăng khi bạn Duyệt (hoặc bật chế độ live)."
              />
            )}
            {sections[active]}
          </div>
        </Content>
      </Layout>
    </Layout>
  )
}

function GuideCard({ redirectUri }: { redirectUri: string }) {
  return (
    <Card size="small" title="📖 Hướng dẫn hoạt động">
      <Paragraph type="secondary" style={{ marginTop: 0 }}>
        Facebook Pro quản lý Fanpage qua <b>Facebook Graph API chính thức</b> bằng chính Developer App của bạn.
        Mọi thao tác đăng/trả lời theo kiểu <b>draft-first</b> — soạn vào hàng chờ, chỉ đăng khi bạn Duyệt
        (trừ khi bật chế độ <Text code>live</Text>).
      </Paragraph>
      <Alert
        type="warning"
        showIcon
        style={{ marginBottom: 16 }}
        message="Trên app desktop: các link (cấp quyền OAuth, tài liệu) sẽ mở ở TRÌNH DUYỆT HỆ THỐNG (Facebook không cho đăng nhập trong webview nhúng). Sau khi cấp quyền xong, quay lại app — trạng thái tự cập nhật sau vài giây."
      />
      <Steps
        direction="vertical"
        size="small"
        current={-1}
        items={[
          {
            title: 'Tạo Facebook Developer App',
            description: <span>Tại developers.facebook.com/apps → tạo App <b>Business</b> → thêm <Text code>Facebook Login</Text> → whitelist redirect <Text code>{redirectUri}</Text>.</span>,
          },
          {
            title: 'Nhập App ID + App Secret',
            description: 'Lấy ở App Settings → Basic, dán vào mục 1 bên dưới rồi bấm Lưu (App Secret chỉ lưu cục bộ).',
          },
          {
            title: 'Cấp quyền',
            description: 'Cách A: bấm "Mở link cấp quyền (OAuth)" → đăng nhập ở trình duyệt. Cách B: dán User Access Token từ Graph API Explorer.',
          },
          {
            title: 'Chọn Trang',
            description: 'Sang tab Trang → chọn Fanpage muốn thao tác (Trang đang chọn = active page cho mọi tác vụ).',
          },
          {
            title: 'Đăng bài & tương tác',
            description: 'Tab Đăng bài (chữ/link/ảnh URL hoặc ảnh từ máy), Bài & bình luận (đọc/trả lời/like), Tin nhắn (Messenger). Nháp vào tab Hàng chờ duyệt để Duyệt & đăng.',
          },
          {
            title: 'Tự động hoá',
            description: 'Đặt chế độ tự chủ (observe/draft/live) ở mục 3; thêm Trigger để auto-soạn trả lời bình luận mới theo từ khoá/câu hỏi.',
          },
          {
            title: 'Thống kê & Quảng cáo',
            description: 'Tab Tổng quan (tương tác), Thống kê & phân tích (Insights + AI), Quảng cáo (CTR/CPC/CPM/ROAS + AI đánh giá đốt tiền / nên tắt).',
          },
        ]}
      />
    </Card>
  )
}

function ConnectTab({ onChange }: { onChange: () => void }) {
  const [settings, setSettings] = useState<SettingsPublic | null>(null)
  const [form] = Form.useForm()
  const [token, setToken] = useState('')

  const load = () =>
    api.getSettings().then((s) => {
      setSettings(s)
      form.setFieldsValue({ app_id: s.app_id, version: s.version })
    })
  useEffect(() => { load() }, [])

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
      openExternal(r.url)
      message.info('Đã mở link cấp quyền ở trình duyệt hệ thống — admin bấm Đồng ý, xong quay lại app')
    } else message.error(r.error || 'Chưa cấu hình App ID/App Secret')
  }
  const connectByToken = async () => {
    if (!token.trim()) return message.warning('Dán User Access Token trước')
    const r: any = await api.connectToken(token.trim())
    if (r.error) message.error(String(r.error))
    else { message.success(`Đã kết nối ${r.pages} Trang`); setToken(''); onChange() }
  }
  const setAutonomy = async (val: string) => { await api.setSettings({ autonomy: val }); onChange(); load() }

  const redirectUri = `${location.origin}/api/oauth/callback`
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <GuideCard redirectUri={redirectUri} />

      <Card
        title="1. Facebook Developer App"
        size="small"
        extra={
          <Button size="small" icon={<LinkOutlined />} onClick={() => openExternal('https://developers.facebook.com/apps')}>
            Mở Developers Console
          </Button>
        }
      >
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Tạo một App loại Business, thêm sản phẩm <Text code>Facebook Login</Text>, whitelist redirect URI{' '}
          <Text code copyable>{redirectUri}</Text>, rồi lấy <Text code>App ID</Text> +{' '}
          <Text code>App Secret</Text> ở App Settings → Basic. <Text code>App Secret</Text> chỉ lưu cục bộ.
        </Paragraph>
        <Form form={form} layout="vertical" onFinish={save}>
          <Flex gap={12} wrap>
            <Form.Item name="app_id" label="App ID" style={{ flex: 1, minWidth: 180 }}>
              <Input placeholder="1234567890" />
            </Form.Item>
            <Form.Item
              name="app_secret"
              label={settings?.app_secret_set ? 'App Secret (đã đặt — nhập lại để đổi)' : 'App Secret'}
              style={{ flex: 1, minWidth: 220 }}
            >
              <Input.Password placeholder={settings?.app_secret_set ? '••••••••' : 'app secret'} />
            </Form.Item>
            <Form.Item name="version" label="Graph version" style={{ width: 140 }}>
              <Input placeholder="v21.0" />
            </Form.Item>
          </Flex>
          <Button type="primary" htmlType="submit">Lưu</Button>
        </Form>
      </Card>

      <Card title="2. Cấp quyền (OAuth) — chọn một cách" size="small">
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Cách A: đăng nhập OAuth để cấp quyền các Trang (scopes: pages_show_list, pages_manage_posts,
          pages_read_engagement, pages_manage_engagement, pages_read_user_content, read_insights).
        </Paragraph>
        <Button onClick={authorize}>Mở link cấp quyền (OAuth)</Button>
        <Paragraph type="secondary" style={{ margin: '16px 0 8px' }}>
          Cách B: dán User Access Token từ{' '}
          <a onClick={() => openExternal('https://developers.facebook.com/tools/explorer/')}>Graph API Explorer</a>
          {' '}— app tự đổi sang token dài hạn & lấy Trang.
        </Paragraph>
        <Flex gap={8}>
          <Input.Password value={token} onChange={(e) => setToken(e.target.value)} placeholder="EAAB..." />
          <Button onClick={connectByToken}>Kết nối bằng token</Button>
        </Flex>
        {settings?.user_token_set && <Tag color="green" style={{ marginTop: 10 }}>user token đã lưu</Tag>}
      </Card>

      <Card title="3. Chế độ tự chủ (autonomy)" size="small">
        <Segmented
          value={settings?.autonomy ?? 'draft'}
          onChange={(v) => setAutonomy(String(v))}
          options={[
            { label: 'Observe (chỉ đọc)', value: 'observe' },
            { label: 'Draft (soạn, chờ duyệt)', value: 'draft' },
            { label: 'Live (tự đăng)', value: 'live' },
          ]}
        />
        <Paragraph type="secondary" style={{ marginBottom: 0, marginTop: 8 }}>
          <b>Draft</b> (khuyến nghị): heartbeat soạn sẵn trả lời cho bình luận khớp trigger, bạn duyệt mới đăng.
        </Paragraph>
      </Card>
    </Space>
  )
}

function PagesTab({ active, onChange }: { active?: string; onChange: () => void }) {
  const [pages, setPages] = useState<Page[]>([])
  const [activeId, setActiveId] = useState(active || '')
  const load = () => api.pages().then((r) => { setPages(r.pages); setActiveId(r.active_page_id) }).catch(() => {})
  useEffect(() => { load() }, [])

  const select = async (id: string) => {
    const r: any = await api.selectPage(id)
    if (r.error) message.error(String(r.error))
    else { message.success('Đã chọn Trang'); setActiveId(id); onChange() }
  }
  if (!pages.length) return <Empty description="Chưa có Trang nào — kết nối ở tab Kết nối trước" />
  return (
    <List
      dataSource={pages}
      renderItem={(p) => (
        <List.Item
          actions={[
            p.page_id === activeId
              ? <Tag color="blue" key="a">Đang chọn</Tag>
              : <Button key="s" size="small" onClick={() => select(p.page_id)}>Chọn</Button>,
          ]}
        >
          <List.Item.Meta
            title={<Space><Text strong>{p.name}</Text><Text type="secondary">#{p.page_id}</Text></Space>}
            description={p.category}
          />
        </List.Item>
      )}
    />
  )
}

function ComposeTab({ onChange }: { onChange: () => void }) {
  const [form] = Form.useForm()
  const [uploading, setUploading] = useState(false)
  const submit = async (v: any) => {
    const r: any = await api.createPost({ message: v.message, link: v.link || undefined, image_url: v.image_url || undefined })
    if (r.error) message.error(String(r.error))
    else message.success(r.status === 'published' ? 'Đã đăng (live)' : `Đã tạo nháp #${r.draft_id}`)
    onChange()
  }
  const uploadLocal = async (file: File) => {
    setUploading(true)
    const r: any = await api.uploadPhoto(file, form.getFieldValue('message') || '')
    setUploading(false)
    if (r.error) message.error(String(r.error))
    else message.success(r.status === 'published' ? 'Đã đăng ảnh (live)' : `Đã tạo nháp ảnh #${r.draft_id}`)
    onChange()
    return false // prevent AntD default upload
  }
  return (
    <Card title="Soạn bài đăng (draft-first)" size="small">
      <Paragraph type="secondary" style={{ marginTop: 0 }}>
        Đăng lên Trang đang chọn. Đính kèm ảnh bằng <b>URL ảnh</b> công khai hoặc <b>tải ảnh từ máy</b>;
        có <Text code>link</Text> → đăng kèm link. Mặc định vào hàng chờ duyệt.
      </Paragraph>
      <Form form={form} layout="vertical" onFinish={submit}>
        <Form.Item name="message" label="Nội dung / chú thích ảnh" rules={[{ required: true }]}>
          <Input.TextArea rows={4} placeholder="Viết gì đó cho fanpage…" />
        </Form.Item>
        <Flex gap={12} wrap>
          <Form.Item name="link" label="Link (tuỳ chọn)" style={{ flex: 1, minWidth: 220 }}>
            <Input placeholder="https://…" />
          </Form.Item>
          <Form.Item name="image_url" label="URL ảnh (tuỳ chọn — đăng bài ảnh)" style={{ flex: 1, minWidth: 220 }}>
            <Input placeholder="https://…/anh.jpg" />
          </Form.Item>
        </Flex>
        <Flex gap={12} align="center">
          <Button type="primary" htmlType="submit">Tạo nháp (chữ/link/URL ảnh)</Button>
          <Upload accept="image/*" showUploadList={false} beforeUpload={uploadLocal} maxCount={1}>
            <Button icon={<UploadOutlined />} loading={uploading}>Tải ảnh từ máy & tạo nháp</Button>
          </Upload>
        </Flex>
      </Form>
    </Card>
  )
}

function PostsTab({ onChange }: { onChange: () => void }) {
  const [posts, setPosts] = useState<any[]>([])
  const [err, setErr] = useState('')
  const [sel, setSel] = useState<string>('')
  const [comments, setComments] = useState<any[]>([])
  const load = () =>
    api.posts().then((r: any) => { if (r.error) setErr(String(r.error)); else { setErr(''); setPosts(r.data || []) } }).catch(() => {})
  useEffect(() => { load() }, [])

  const openComments = async (postId: string) => {
    setSel(postId)
    const r: any = await api.comments(postId)
    setComments(r.data || [])
  }
  const reply = async (commentId: string, text: string) => {
    const r: any = await api.reply({ comment_id: commentId, comment_text: text })
    if (r.error) message.error(String(r.error))
    else message.success(r.status === 'published' ? 'Đã trả lời (live)' : `Đã tạo nháp trả lời #${r.draft_id}`)
    onChange()
  }
  const like = async (id: string) => {
    const r: any = await api.like({ object_id: id })
    if (r.error) message.error(String(r.error)); else message.success('Đã like')
  }
  const del = async (id: string) => {
    const r: any = await api.deletePost({ post_id: id })
    if (r.error) message.error(String(r.error)); else { message.success('Đã xoá'); load() }
  }

  if (err) return <Alert type="warning" showIcon message={err} action={<Button size="small" onClick={load}>Thử lại</Button>} />
  return (
    <Flex gap={16} align="start" wrap>
      <Card title="Bài đăng gần đây" size="small" style={{ flex: 1, minWidth: 340 }} extra={<Button size="small" onClick={load}>Làm mới</Button>}>
        {!posts.length ? <Empty description="Chưa có bài" /> : (
          <List
            dataSource={posts}
            renderItem={(p) => (
              <List.Item
                actions={[
                  <a key="c" onClick={() => openComments(p.id)}>Bình luận</a>,
                  <a key="l" onClick={() => like(p.id)}>Like</a>,
                  <a key="d" style={{ color: '#ff4d4f' }} onClick={() => del(p.id)}>Xoá</a>,
                ]}
              >
                <List.Item.Meta
                  title={<Text ellipsis style={{ maxWidth: 300 }}>{p.message || p.story || '(không có text)'}</Text>}
                  description={<Space size="small">
                    <Tag>❤ {p.reactions?.summary?.total_count ?? 0}</Tag>
                    <Tag>💬 {p.comments?.summary?.total_count ?? 0}</Tag>
                    <Tag>↗ {p.shares?.count ?? 0}</Tag>
                  </Space>}
                />
              </List.Item>
            )}
          />
        )}
      </Card>
      <Card title={sel ? `Bình luận · ${sel}` : 'Bình luận'} size="small" style={{ flex: 1, minWidth: 340 }}>
        {!sel ? <Empty description="Chọn 'Bình luận' ở một bài" /> : !comments.length ? <Empty description="Chưa có bình luận" /> : (
          <List
            dataSource={comments}
            renderItem={(c) => (
              <List.Item actions={[<Button key="r" size="small" onClick={() => reply(c.id, c.message)}>Trả lời (AI)</Button>]}>
                <List.Item.Meta
                  title={<Text strong>{c.from?.name || 'Người dùng'}</Text>}
                  description={c.message}
                />
              </List.Item>
            )}
          />
        )}
      </Card>
    </Flex>
  )
}

function OverviewTab() {
  const [data, setData] = useState<any>(null)
  const [err, setErr] = useState('')
  const load = () => api.overview().then((r: any) => { if (r.error) setErr(String(r.error)); else { setErr(''); setData(r) } }).catch(() => {})
  useEffect(() => { load() }, [])

  if (err) return <Alert type="warning" showIcon message={err} action={<Button size="small" onClick={load}>Thử lại</Button>} />
  const t = data?.totals || {}
  const kpi = (label: string, value: any, color: string) => (
    <Card size="small" style={{ flex: 1, minWidth: 130, textAlign: 'center' }}>
      <div style={{ fontSize: 26, fontWeight: 700, color }}>{value ?? '—'}</div>
      <Text type="secondary">{label}</Text>
    </Card>
  )
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Flex justify="space-between" align="center">
        <Text type="secondary">Tổng quan {t.posts ?? 0} bài gần đây của Trang đang chọn</Text>
        <Button size="small" onClick={load}>Làm mới</Button>
      </Flex>
      <Flex gap={12} wrap>
        {kpi('Bài đăng', t.posts, '#1877f2')}
        {kpi('Cảm xúc', t.reactions, '#e4405f')}
        {kpi('Bình luận', t.comments, '#0a7')}
        {kpi('Chia sẻ', t.shares, '#f90')}
        {kpi('Tổng tương tác', t.engagement, '#1877f2')}
        {kpi('Nháp chờ', t.pending_drafts, '#faad14')}
      </Flex>
      <Card title="Top bài theo tương tác" size="small">
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={data?.top_posts || []}
          locale={{ emptyText: 'Chưa có dữ liệu' }}
          columns={[
            { title: 'Nội dung', dataIndex: 'message', render: (v) => <Text ellipsis style={{ maxWidth: 340 }}>{v || '(không có text)'}</Text> },
            { title: '❤', dataIndex: 'reactions', width: 70 },
            { title: '💬', dataIndex: 'comments', width: 70 },
            { title: '↗', dataIndex: 'shares', width: 70 },
            { title: 'Tổng', dataIndex: 'engagement', width: 80, render: (v) => <Tag color="blue">{v}</Tag> },
          ]}
        />
      </Card>
    </Space>
  )
}

function InboxTab({ onChange }: { onChange: () => void }) {
  const [convs, setConvs] = useState<any[]>([])
  const [err, setErr] = useState('')
  const [sel, setSel] = useState<any>(null)
  const [messages, setMessages] = useState<any[]>([])
  const [reply, setReply] = useState('')
  const load = () => api.conversations().then((r: any) => { if (r.error) setErr(String(r.error)); else { setErr(''); setConvs(r.data || []) } }).catch(() => {})
  useEffect(() => { load() }, [])

  const openThread = async (c: any) => {
    setSel(c); setMessages([])
    const r: any = await api.conversationMessages(c.id)
    setMessages(r.messages?.data || [])
  }
  const otherId = (c: any) => c?.participants?.data?.find((p: any) => p.name || p.id)?.id
  const send = async () => {
    const rid = otherId(sel)
    if (!rid) return message.error('Không xác định được người nhận')
    const r: any = await api.messageReply({ recipient_id: rid, message: reply || undefined, customer_msg: sel?.snippet })
    if (r.error) message.error(String(r.error))
    else { message.success(r.status === 'published' ? 'Đã gửi (live)' : `Đã tạo nháp #${r.draft_id}`); setReply('') }
    onChange()
  }

  if (err) return <Alert type="warning" showIcon message={err} action={<Button size="small" onClick={load}>Thử lại</Button>} description="Tin nhắn cần quyền pages_messaging trên Developer App." />
  return (
    <Flex gap={16} align="start" wrap>
      <Card title="Hội thoại" size="small" style={{ flex: 1, minWidth: 300 }} extra={<Button size="small" onClick={load}>Làm mới</Button>}>
        {!convs.length ? <Empty description="Chưa có hội thoại" /> : (
          <List
            dataSource={convs}
            renderItem={(c) => (
              <List.Item onClick={() => openThread(c)} style={{ cursor: 'pointer', background: sel?.id === c.id ? 'rgba(24,119,242,0.12)' : undefined }}>
                <List.Item.Meta
                  title={<Space>{c.participants?.data?.map((p: any) => p.name).filter(Boolean).join(', ') || 'Người dùng'}{c.unread_count > 0 && <Badge count={c.unread_count} size="small" />}</Space>}
                  description={<Text ellipsis style={{ maxWidth: 260 }}>{c.snippet}</Text>}
                />
              </List.Item>
            )}
          />
        )}
      </Card>
      <Card title={sel ? 'Nội dung hội thoại' : 'Chọn một hội thoại'} size="small" style={{ flex: 1, minWidth: 300 }}>
        {!sel ? <Empty description="Chọn hội thoại để xem tin nhắn" /> : (
          <Space direction="vertical" style={{ width: '100%' }}>
            <List
              size="small"
              dataSource={messages}
              locale={{ emptyText: 'Không tải được tin nhắn' }}
              renderItem={(m) => (
                <List.Item>
                  <List.Item.Meta title={<Text strong style={{ fontSize: 12 }}>{m.from?.name || m.from?.id}</Text>} description={m.message} />
                </List.Item>
              )}
            />
            <Input.TextArea rows={2} value={reply} onChange={(e) => setReply(e.target.value)} placeholder="Trả lời (bỏ trống = AI soạn từ tin gần nhất)…" />
            <Button type="primary" onClick={send}>Tạo nháp trả lời</Button>
          </Space>
        )}
      </Card>
    </Flex>
  )
}

function AnalyticsTab() {
  const [postId, setPostId] = useState('')
  const [analysis, setAnalysis] = useState('')
  const [busy, setBusy] = useState(false)
  const [insights, setInsights] = useState<any>(null)

  const analyze = async () => {
    setBusy(true)
    const r: any = await api.analyze({ post_id: postId || undefined })
    setBusy(false)
    if (r.error) message.error(String(r.error)); else setAnalysis(r.analysis || '')
  }
  const loadPageInsights = async () => setInsights(await api.pageInsights())
  const loadPostInsights = async () => { if (postId) setInsights(await api.postInsights(postId)) }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="Phân tích bài viết bằng AI" size="small">
        <Flex gap={8} style={{ marginBottom: 12 }}>
          <Input value={postId} onChange={(e) => setPostId(e.target.value)} placeholder="post_id (vd 12345_67890) — bỏ trống để phân tích text" />
          <Button type="primary" loading={busy} onClick={analyze}>Phân tích</Button>
        </Flex>
        {analysis && <Alert type="success" message={<pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{analysis}</pre>} />}
      </Card>
      <Card title="Insights (thống kê chính thức)" size="small">
        <Space style={{ marginBottom: 12 }}>
          <Button onClick={loadPageInsights}>Insights Trang</Button>
          <Button onClick={loadPostInsights} disabled={!postId}>Insights bài (theo post_id trên)</Button>
        </Space>
        <pre className="jsonbox" style={{ maxHeight: 420 }}>
          {insights ? JSON.stringify(insights, null, 2) : '…'}
        </pre>
      </Card>
    </Space>
  )
}

function AdsTab() {
  const [accounts, setAccounts] = useState<any[]>([])
  const [active, setActive] = useState('')
  const [level, setLevel] = useState('campaign')
  const [datePreset, setDatePreset] = useState('last_7d')
  const [rows, setRows] = useState<any[]>([])
  const [verdict, setVerdict] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  const loadAccounts = async () => {
    const r = await api.adAccounts()
    if (r.error) { setErr(String(r.error)); return }
    setErr(''); setAccounts(r.accounts || []); setActive(r.active_ad_account || (r.accounts?.[0]?.id ?? ''))
  }
  useEffect(() => { loadAccounts() }, [])

  const pickAccount = async (id: string) => { await api.selectAdAccount(id); setActive(id) }
  const loadInsights = async () => {
    setBusy(true); setVerdict('')
    const r: any = await api.adsInsights({ object_id: active, level, date_preset: datePreset })
    setBusy(false)
    if (r.error) { setErr(String(r.error)); setRows([]) } else { setErr(''); setRows(r.rows || []) }
  }
  const analyze = async () => {
    setBusy(true)
    const cur = accounts.find((a) => a.id === active)?.currency || 'VND'
    const r: any = await api.adsAnalyze({ object_id: active, level, date_preset: datePreset, currency: cur })
    setBusy(false)
    if (r.error) setErr(String(r.error)); else { setErr(''); setRows(r.rows || []); setVerdict(r.verdict || '') }
  }

  const verdictTag = (row: any) => {
    const ctr = parseFloat(row.ctr || '0')
    const results = parseFloat(row.results || '0')
    const roas = parseFloat(row.roas || '0')
    const spend = parseFloat(row.spend || '0')
    if (spend > 0 && results === 0) return <Tag color="red">❌ Đốt tiền?</Tag>
    if (roas > 0 && roas < 1) return <Tag color="red">❌ ROAS&lt;1</Tag>
    if (ctr > 0 && ctr < 0.8) return <Tag color="gold">⚠️ CTR thấp</Tag>
    return <Tag color="green">✅ OK</Tag>
  }
  const pause = async (id: string) => {
    const r: any = await api.adStatus({ entity_id: id, status: 'PAUSED' })
    if (r.error) message.error(String(r.error)); else message.success('Đã tắt quảng cáo')
  }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="Tài khoản quảng cáo" size="small" extra={<Button size="small" onClick={loadAccounts}>Làm mới</Button>}>
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Cần user token có quyền <Text code>ads_read</Text> (và <Text code>ads_management</Text> để tắt/bật QC).
        </Paragraph>
        {err && <Alert type="warning" showIcon style={{ marginBottom: 12 }} message={err} />}
        <Flex gap={8} wrap align="center">
          <Select
            style={{ minWidth: 280 }}
            value={active || undefined}
            placeholder="Chọn tài khoản QC"
            onChange={pickAccount}
            options={accounts.map((a) => ({ value: a.id, label: `${a.name || a.id} (${a.currency || '?'})` }))}
          />
          <Select value={level} onChange={setLevel} style={{ width: 150 }} options={[
            { value: 'account', label: 'Toàn tài khoản' },
            { value: 'campaign', label: 'Theo chiến dịch' },
            { value: 'adset', label: 'Theo nhóm QC' },
            { value: 'ad', label: 'Theo quảng cáo' },
          ]} />
          <Select value={datePreset} onChange={setDatePreset} style={{ width: 140 }} options={[
            { value: 'today', label: 'Hôm nay' },
            { value: 'last_7d', label: '7 ngày' },
            { value: 'last_30d', label: '30 ngày' },
            { value: 'maximum', label: 'Tối đa' },
          ]} />
          <Button onClick={loadInsights} loading={busy}>Xem chỉ số</Button>
          <Button type="primary" onClick={analyze} loading={busy}>Phân tích AI</Button>
        </Flex>
      </Card>

      {verdict && (
        <Alert type="info" showIcon message="Đánh giá của AI"
          description={<pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{verdict}</pre>} />
      )}

      <Table
        size="small"
        rowKey={(r) => r.name}
        dataSource={rows}
        pagination={false}
        scroll={{ x: 900 }}
        locale={{ emptyText: 'Chưa có dữ liệu — bấm "Xem chỉ số" hoặc "Phân tích AI"' }}
        columns={[
          { title: 'Chiến dịch / QC', dataIndex: 'name', fixed: 'left', width: 180 },
          { title: 'Đánh giá', width: 110, render: (_, r) => verdictTag(r) },
          { title: 'Chi tiêu', dataIndex: 'spend', width: 100 },
          { title: 'CTR %', dataIndex: 'ctr', width: 80 },
          { title: 'CPC', dataIndex: 'cpc', width: 90 },
          { title: 'CPM', dataIndex: 'cpm', width: 90 },
          { title: 'Hiển thị', dataIndex: 'impressions', width: 90 },
          { title: 'Click', dataIndex: 'clicks', width: 70 },
          { title: 'Kết quả', width: 100, render: (_, r) => <span>{r.results || '0'}{r.result_type ? <Text type="secondary" style={{ fontSize: 11 }}> ({r.result_type})</Text> : null}</span> },
          { title: 'Chi/KQ', dataIndex: 'cost_per_result', width: 90 },
          { title: 'ROAS', dataIndex: 'roas', width: 70 },
        ]}
      />
      <Paragraph type="secondary" style={{ marginTop: -8 }}>
        Muốn tắt một chiến dịch/QC đang đốt tiền: lấy <Text code>id</Text> ở tab Ads (level = chiến dịch/nhóm/QC),
        rồi dùng công cụ <Text code>fb_ad_status</Text> hoặc nút tắt dưới đây theo id.
      </Paragraph>
      <PauseByIdCard onPause={pause} />
    </Space>
  )
}

function PauseByIdCard({ onPause }: { onPause: (id: string) => void }) {
  const [id, setId] = useState('')
  return (
    <Card size="small" title="Tắt/bật nhanh theo id (thao tác tức thời)">
      <Flex gap={8}>
        <Input value={id} onChange={(e) => setId(e.target.value)} placeholder="campaign_id / adset_id / ad_id" />
        <Button danger onClick={() => id && onPause(id)}>Tắt (PAUSED)</Button>
      </Flex>
    </Card>
  )
}

function TriggersTab() {
  const [triggers, setTriggers] = useState<Trigger[]>([])
  const [form] = Form.useForm()
  const load = () => api.triggers().then((r) => setTriggers(r.triggers)).catch(() => {})
  useEffect(() => { load() }, [])

  const create = async (v: any) => {
    const r: any = await api.createTrigger({
      name: v.name, match_type: v.match_type, match_value: v.match_value, action: v.action, reply_hint: v.reply_hint,
    })
    if (r.error) message.error(String(r.error))
    else { message.success('Đã tạo trigger'); form.resetFields(); load() }
  }
  const del = async (id: number) => { await api.deleteTrigger(id); load() }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card title="Trigger theo luật (bình luận mới → auto-reply / thông báo)" size="small">
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Khi heartbeat thấy bình luận mới khớp luật, nó soạn nháp trả lời (AI) hoặc ghi thông báo. Không tự đăng
          trừ khi autonomy = live.
        </Paragraph>
        <Form form={form} layout="inline" style={{ rowGap: 12 }} onFinish={create}
          initialValues={{ match_type: 'question', action: 'draft_reply' }}>
          <Form.Item name="name" label="Tên" rules={[{ required: true }]}><Input placeholder="Hỏi giá" style={{ width: 140 }} /></Form.Item>
          <Form.Item name="match_type" label="Luật">
            <Select style={{ width: 130 }} options={[
              { value: 'all', label: 'Tất cả' },
              { value: 'keyword', label: 'Từ khoá' },
              { value: 'question', label: 'Câu hỏi' },
            ]} />
          </Form.Item>
          <Form.Item name="match_value" label="Từ khoá (CSV)"><Input placeholder="giá,ship,còn" style={{ width: 160 }} /></Form.Item>
          <Form.Item name="action" label="Hành động">
            <Select style={{ width: 140 }} options={[
              { value: 'draft_reply', label: 'Soạn trả lời' },
              { value: 'notify', label: 'Thông báo' },
            ]} />
          </Form.Item>
          <Form.Item name="reply_hint" label="Gợi ý trả lời"><Input placeholder="mời nhắn tin trang" style={{ width: 180 }} /></Form.Item>
          <Button type="primary" htmlType="submit">Thêm</Button>
        </Form>
      </Card>
      <Table
        size="small"
        rowKey="id"
        dataSource={triggers}
        pagination={false}
        locale={{ emptyText: 'Chưa có trigger' }}
        columns={[
          { title: 'Tên', dataIndex: 'name' },
          { title: 'Luật', dataIndex: 'match_type', render: (v, r) => <span>{v}{r.match_value ? `: ${r.match_value}` : ''}</span> },
          { title: 'Hành động', dataIndex: 'action', render: (v) => <Tag color={v === 'notify' ? 'gold' : 'blue'}>{v}</Tag> },
          { title: 'Bật', dataIndex: 'enabled', render: (v) => <Switch checked={v} disabled size="small" /> },
          { title: '', render: (_, r) => <a style={{ color: '#ff4d4f' }} onClick={() => del(r.id)}>Xoá</a> },
        ]}
      />
    </Space>
  )
}

function DraftsTab({ onChange }: { onChange: () => void }) {
  const [drafts, setDrafts] = useState<Draft[]>([])
  const load = () => api.drafts().then((d) => setDrafts(d.pending)).catch(() => {})
  useEffect(() => { load() }, [])

  const approve = async (id: number) => {
    const r: any = await api.approve(id)
    if (r.error) message.error(String(r.error)); else message.success('Đã đăng')
    load(); onChange()
  }
  const reject = async (id: number) => { await api.reject(id); message.info('Đã bỏ nháp'); load(); onChange() }

  if (!drafts.length) return <Empty description="Không có bản nháp chờ duyệt" />
  return (
    <List
      dataSource={drafts}
      renderItem={(d) => (
        <List.Item actions={[
          <Button key="a" type="primary" size="small" onClick={() => approve(d.id)}>Duyệt & đăng</Button>,
          <Button key="r" size="small" danger onClick={() => reject(d.id)}>Bỏ</Button>,
        ]}>
          <List.Item.Meta
            title={<Space><Tag color="blue">{d.kind}</Tag><Tag>{d.source}</Tag>{d.model && <Tag color="purple">{d.model}</Tag>}{d.target_id && <Text type="secondary">→ {d.target_id}</Text>}</Space>}
            description={<Paragraph style={{ marginBottom: 0 }}>{d.message}{d.image_url && <div><Text type="secondary">🖼 {d.image_url}</Text></div>}{d.link && <div><Text type="secondary">🔗 {d.link}</Text></div>}</Paragraph>}
          />
        </List.Item>
      )}
    />
  )
}

function ActivityTab() {
  const [items, setItems] = useState<any[]>([])
  useEffect(() => { api.activity().then((a) => setItems(a.activity)).catch(() => {}) }, [])
  if (!items.length) return <Empty description="Chưa có hoạt động" />
  return (
    <List
      size="small"
      dataSource={items}
      renderItem={(a) => (
        <List.Item>
          <Space><Tag>{a.kind}</Tag><Text>{a.text}</Text>{a.ref && <Text type="secondary">({a.ref})</Text>}</Space>
        </List.Item>
      )}
    />
  )
}
