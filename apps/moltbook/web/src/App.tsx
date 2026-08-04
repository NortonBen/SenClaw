import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert,
  Avatar,
  Badge,
  Button,
  Card,
  Empty,
  Flex,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Spin,
  Switch,
  Tabs,
  Tag,
  Timeline,
  Tooltip,
  Typography,
  message,
} from 'antd'
import {
  ArrowDownOutlined,
  ArrowUpOutlined,
  BookOutlined,
  CommentOutlined,
  DisconnectOutlined,
  EditOutlined,
  ExperimentOutlined,
  FireOutlined,
  MessageOutlined,
  PlayCircleOutlined,
  QuestionCircleOutlined,
  ReloadOutlined,
  RobotOutlined,
  SendOutlined,
  SyncOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import type {
  Account,
  Activity,
  CachedPost,
  Draft,
  Integrations,
  ModelConfig,
  RecallResult,
  ResearchRunResult,
  ResearchStep,
  ResearchWorkflow,
  ToolCatalog,
  Topic,
  TrackedPost,
  TrendingDigest,
  TrendingRun,
} from './api'
import { api, draftQuestions, draftResearch, fmtDateTime, fmtRelative, hueFromName } from './api'

const { Title, Text, Paragraph } = Typography

const AUTONOMY_OPTS = [
  { label: 'Quan sát', value: 'observe' },
  { label: 'Nháp & duyệt', value: 'draft' },
  { label: 'Tự động', value: 'live' },
]

const KIND_LABEL: Record<string, string> = {
  post: 'Bài đăng',
  comment: 'Bình luận',
  vote: 'Vote',
  submolt: 'Submolt',
  follow: 'Theo dõi',
  subscribe: 'Đăng ký',
}

export default function App() {
  const [account, setAccount] = useState<Account | null>(null)
  const [tab, setTab] = useState('feed')
  const [busyHeartbeat, setBusyHeartbeat] = useState(false)

  const reloadAccount = useCallback(async () => {
    try {
      setAccount(await api.account())
    } catch (e) {
      message.error(`Không tải được trạng thái: ${(e as Error).message}`)
    }
  }, [])

  useEffect(() => {
    reloadAccount()
  }, [reloadAccount])

  const setAutonomy = async (value: string) => {
    try {
      const a = await api.putSettings({ autonomy: value as Account['autonomy'] })
      setAccount(a)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const runHeartbeat = async () => {
    setBusyHeartbeat(true)
    try {
      const r = await api.runHeartbeat()
      if (r.ok) message.success(r.note || 'Heartbeat xong.')
      else message.warning(r.reason || 'Heartbeat không chạy.')
      reloadAccount()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusyHeartbeat(false)
    }
  }

  const karma = (account?.profile?.karma as number | undefined) ?? undefined

  return (
    <div style={{ maxWidth: 940, margin: '0 auto', padding: '20px 16px 60px' }}>
      <Flex align="center" justify="space-between" wrap gap={12} style={{ marginBottom: 16 }}>
        <Flex align="center" gap={10}>
          <span style={{ fontSize: 30 }}>🦞</span>
          <div>
            <Title level={3} style={{ margin: 0 }}>
              Moltbook
            </Title>
            <Text type="secondary" style={{ fontSize: 12 }}>
              the front page of the agent internet
            </Text>
          </div>
        </Flex>
        <Space wrap>
          {account?.connected ? (
            <Tag color="green">
              @{account.agent_name || 'molty'}
              {karma !== undefined ? ` · ${karma} karma` : ''}
            </Tag>
          ) : (
            <Tag>Chưa kết nối</Tag>
          )}
          <Tooltip title="Chế độ tự động: Quan sát = chỉ đọc · Nháp & duyệt = soạn chờ duyệt · Tự động = đăng luôn">
            <Segmented
              size="small"
              value={account?.autonomy || 'draft'}
              options={AUTONOMY_OPTS}
              onChange={(v) => setAutonomy(String(v))}
            />
          </Tooltip>
          <Button
            type="primary"
            icon={<ThunderboltOutlined />}
            loading={busyHeartbeat}
            onClick={runHeartbeat}
            disabled={!account?.connected}
          >
            Heartbeat
          </Button>
        </Space>
      </Flex>

      {account && !account.connected && (
        <Alert
          type="info"
          style={{ marginBottom: 16 }}
          message="Chưa kết nối agent Moltbook"
          description="Đang xem feed DEMO. Vào tab Cài đặt để đăng ký một molty mới hoặc dán API key có sẵn."
          action={
            <Button size="small" onClick={() => setTab('settings')}>
              Cài đặt
            </Button>
          }
        />
      )}

      <Tabs
        activeKey={tab}
        onChange={setTab}
        items={[
          { key: 'feed', label: 'Feed', children: <FeedTab account={account} onChanged={reloadAccount} /> },
          {
            key: 'drafts',
            label: (
              <Badge
                count={(account?.pending_drafts || 0) + (account?.needs_input_drafts || 0)}
                size="small"
                offset={[10, -2]}
              >
                Hàng chờ duyệt
              </Badge>
            ),
            children: <DraftsTab needsInput={account?.needs_input_drafts || 0} onChanged={reloadAccount} />,
          },
          {
            key: 'research',
            label: 'Nghiên cứu',
            children: <ResearchTab active={tab === 'research'} account={account} onChanged={reloadAccount} />,
          },
          { key: 'trending', label: 'Xu hướng', children: <TrendingTab active={tab === 'trending'} account={account} /> },
          {
            key: 'myposts',
            label: 'Bài của tôi',
            children: <MyPostsTab active={tab === 'myposts'} account={account} onChanged={reloadAccount} />,
          },
          { key: 'activity', label: 'Nhật ký', children: <ActivityTab active={tab === 'activity'} /> },
          {
            key: 'settings',
            label: 'Cài đặt',
            children: <SettingsTab account={account} onChanged={reloadAccount} />,
          },
        ]}
      />
    </div>
  )
}

// ---------------------------------------------------------------- Feed

function FeedTab({ account, onChanged }: { account: Account | null; onChanged: () => void }) {
  const [feed, setFeed] = useState<CachedPost[]>([])
  const [source, setSource] = useState<string>('')
  const [loading, setLoading] = useState(false)
  const [sort, setSort] = useState('hot')
  const [replyFor, setReplyFor] = useState<CachedPost | null>(null)
  const [newPostOpen, setNewPostOpen] = useState(false)

  const load = useCallback(
    async (refresh: boolean) => {
      setLoading(true)
      try {
        const r = await api.feed({ sort, refresh })
        setFeed(r.posts)
        setSource(r.source)
        if (r.warning) message.warning(`Moltbook: ${r.warning}`)
      } catch (e) {
        message.error((e as Error).message)
      } finally {
        setLoading(false)
      }
    },
    [sort],
  )

  useEffect(() => {
    load(false)
  }, [load])

  const afterAction = (r: Record<string, unknown>) => {
    if (r.gated === 'observe') message.warning(String(r.message))
    else if (r.published) message.success('Đã đăng lên Moltbook.')
    else if (r.queued) message.success('Đã đưa vào hàng chờ duyệt.')
    onChanged()
  }

  const vote = async (p: CachedPost, dir: 'up' | 'down') => {
    try {
      afterAction(await api.vote(p.post_id, dir, p.title))
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const archive = async (p: CachedPost) => {
    const hide = message.loading('Đang lưu vào kho thông tin…', 0)
    try {
      const r = await api.wikiArchive(p.post_id)
      hide()
      message.success(`Đã lưu vào wiki: ${r.path}`)
    } catch (e) {
      hide()
      message.error((e as Error).message)
    }
  }

  const aiReply = async (p: CachedPost) => {
    const hide = message.loading('Đang soạn trả lời…', 0)
    try {
      await api.composeReply({ target_post_id: p.post_id, post_title: p.title, post_content: p.content })
      hide()
      message.success('Đã soạn nháp trả lời — kiểm tra tab Hàng chờ duyệt.')
      onChanged()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    }
  }

  return (
    <div>
      <Flex justify="space-between" align="center" wrap gap={8} style={{ marginBottom: 12 }}>
        <Segmented
          value={sort}
          onChange={(v) => setSort(String(v))}
          options={[
            { label: '🔥 Hot', value: 'hot' },
            { label: '🆕 Mới', value: 'new' },
            { label: '⬆ Top', value: 'top' },
          ]}
        />
        <Space>
          {source === 'demo' && <Tag color="gold">DEMO</Tag>}
          {source === 'live' && <Tag color="green">LIVE</Tag>}
          <Button icon={<EditOutlined />} onClick={() => setNewPostOpen(true)}>
            Bài mới
          </Button>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={() => load(true)}>
            Làm mới
          </Button>
        </Space>
      </Flex>

      <Spin spinning={loading}>
        {feed.length === 0 ? (
          <Empty description="Chưa có bài nào" />
        ) : (
          <Space direction="vertical" size={12} style={{ width: '100%' }}>
            {feed.map((p) => (
              <Card key={p.post_id} size="small" styles={{ body: { padding: 14 } }}>
                <Flex gap={12} align="flex-start">
                  <Flex vertical align="center" gap={2} style={{ minWidth: 40 }}>
                    <Button type="text" size="small" icon={<ArrowUpOutlined />} onClick={() => vote(p, 'up')} />
                    <Text strong>{p.score}</Text>
                    <Button type="text" size="small" icon={<ArrowDownOutlined />} onClick={() => vote(p, 'down')} />
                  </Flex>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Space size={6} wrap style={{ marginBottom: 4 }}>
                      <Tag color="volcano" style={{ margin: 0 }}>
                        {p.submolt}
                      </Tag>
                      <Avatar size={18} style={{ backgroundColor: `hsl(${hueFromName(p.author)},60%,45%)`, fontSize: 10 }}>
                        {p.author.slice(0, 1).toUpperCase()}
                      </Avatar>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {p.author} · {fmtRelative(p.posted_at)}
                      </Text>
                      {p.demo && <Tag color="gold">demo</Tag>}
                    </Space>
                    <div style={{ fontWeight: 600, fontSize: 15, marginBottom: 4 }}>{p.title}</div>
                    {p.content && (
                      <Paragraph type="secondary" ellipsis={{ rows: 3, expandable: true, symbol: 'thêm' }} style={{ marginBottom: 8 }}>
                        {p.content}
                      </Paragraph>
                    )}
                    <Space wrap>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        <CommentOutlined /> {p.comment_count}
                      </Text>
                      <Button size="small" type="text" icon={<RobotOutlined />} onClick={() => aiReply(p)} disabled={!account?.connected}>
                        Nháp trả lời (AI)
                      </Button>
                      <Button size="small" type="text" icon={<MessageOutlined />} onClick={() => setReplyFor(p)}>
                        Trả lời
                      </Button>
                      <Tooltip title="Lưu bài + thảo luận vào wiki (kho thông tin)">
                        <Button
                          size="small"
                          type="text"
                          icon={<BookOutlined />}
                          onClick={() => archive(p)}
                          disabled={!account?.connected}
                        >
                          Lưu vào wiki
                        </Button>
                      </Tooltip>
                    </Space>
                  </div>
                </Flex>
              </Card>
            ))}
          </Space>
        )}
      </Spin>

      <ReplyModal post={replyFor} onClose={() => setReplyFor(null)} onDone={afterAction} />
      <NewPostModal open={newPostOpen} defaultSubmolt={account?.default_submolt || 'general'} onClose={() => setNewPostOpen(false)} onDone={afterAction} />
    </div>
  )
}

function ReplyModal({
  post,
  onClose,
  onDone,
}: {
  post: CachedPost | null
  onClose: () => void
  onDone: (r: Record<string, unknown>) => void
}) {
  const [content, setContent] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (post) setContent('')
  }, [post])
  const submit = async () => {
    if (!post || !content.trim()) return
    setBusy(true)
    try {
      onDone(await api.comment(post.post_id, content.trim(), post.title))
      onClose()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal
      title={post ? `Trả lời: ${post.title}` : ''}
      open={!!post}
      onCancel={onClose}
      onOk={submit}
      okText="Vào hàng chờ"
      confirmLoading={busy}
      okButtonProps={{ disabled: !content.trim() }}
    >
      <Input.TextArea rows={4} value={content} onChange={(e) => setContent(e.target.value)} placeholder="Viết bình luận của bạn…" />
    </Modal>
  )
}

function NewPostModal({
  open,
  defaultSubmolt,
  onClose,
  onDone,
}: {
  open: boolean
  defaultSubmolt: string
  onClose: () => void
  onDone: (r: Record<string, unknown>) => void
}) {
  const [submolt, setSubmolt] = useState(defaultSubmolt)
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (open) {
      setSubmolt(defaultSubmolt)
      setTitle('')
      setContent('')
    }
  }, [open, defaultSubmolt])

  const submit = async () => {
    if (!title.trim()) return
    setBusy(true)
    try {
      onDone(await api.createPost({ submolt, title: title.trim(), content }))
      onClose()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const aiDraft = async () => {
    setBusy(true)
    const hide = message.loading('AI đang soạn bài…', 0)
    try {
      const r = await api.composePost({ submolt, topic: title })
      hide()
      setTitle(r.draft.title)
      setContent(r.draft.content)
      message.success('Đã soạn — chỉnh lại nếu cần rồi bấm Vào hàng chờ.')
    } catch (e) {
      hide()
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      title="Bài đăng mới"
      open={open}
      onCancel={onClose}
      onOk={submit}
      okText="Vào hàng chờ"
      confirmLoading={busy}
      okButtonProps={{ disabled: !title.trim() }}
    >
      <Space direction="vertical" style={{ width: '100%' }}>
        <Input addonBefore="m/" value={submolt} onChange={(e) => setSubmolt(e.target.value)} placeholder="submolt" />
        <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Tiêu đề (hoặc chủ đề để AI soạn)" maxLength={300} />
        <Input.TextArea rows={5} value={content} onChange={(e) => setContent(e.target.value)} placeholder="Nội dung…" />
        <Button icon={<RobotOutlined />} onClick={aiDraft} loading={busy}>
          AI soạn giúp
        </Button>
      </Space>
    </Modal>
  )
}

// ---------------------------------------------------------------- Drafts

function DraftsTab({ needsInput, onChanged }: { needsInput: number; onChanged: () => void }) {
  const [drafts, setDrafts] = useState<Draft[]>([])
  const [filter, setFilter] = useState('pending')
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setDrafts(await api.listDrafts(filter === 'all' ? undefined : filter))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [filter])

  useEffect(() => {
    load()
  }, [load])

  const approve = async (d: Draft) => {
    const hide = message.loading('Đang đăng lên Moltbook…', 0)
    try {
      const r = await api.approveDraft(d.id)
      hide()
      if (r.ok) message.success('Đã đăng lên Moltbook.')
      else message.error(`Lỗi: ${r.error}`)
      load()
      onChanged()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    }
  }
  const reject = async (d: Draft) => {
    try {
      await api.rejectDraft(d.id)
      load()
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  // The post is already live on Moltbook; only its challenge failed. Retrying
  // verification must never re-post.
  const retryVerify = async (d: Draft) => {
    const hide = message.loading('Đang giải lại challenge…', 0)
    try {
      const r = await api.verifyDraft(d.id)
      hide()
      if (r.ok) message.success('Đã xác minh — bài đã lên Moltbook.')
      else message.error(r.error || 'Xác minh lại thất bại.')
      load()
      onChanged()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    }
  }

  return (
    <div>
      <Flex justify="space-between" align="center" style={{ marginBottom: 12 }} wrap gap={8}>
        <Segmented
          value={filter}
          onChange={(v) => setFilter(String(v))}
          options={[
            { label: 'Chờ duyệt', value: 'pending' },
            {
              label: needsInput > 0 ? `Cần trả lời (${needsInput})` : 'Cần trả lời',
              value: 'needs_input',
            },
            { label: 'Đã đăng', value: 'posted' },
            { label: 'Từ chối', value: 'rejected' },
            { label: 'Lỗi', value: 'error' },
            { label: 'Tất cả', value: 'all' },
          ]}
        />
        <Button icon={<ReloadOutlined />} loading={loading} onClick={load}>
          Làm mới
        </Button>
      </Flex>

      <Spin spinning={loading}>
        {drafts.length === 0 ? (
          <Empty description="Không có bản nháp nào" />
        ) : (
          <Space direction="vertical" size={10} style={{ width: '100%' }}>
            {drafts.map((d) => (
              <Card key={d.id} size="small">
                <Flex justify="space-between" align="flex-start" gap={12} wrap>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Space size={6} wrap style={{ marginBottom: 4 }}>
                      <Tag color="volcano">{KIND_LABEL[d.kind] || d.kind}</Tag>
                      <StatusTag status={d.status} />
                      {d.verify_post_id && d.verify_code && <Tag color="orange">chờ xác minh</Tag>}
                      {d.source === 'engine' && <Tag>heartbeat</Tag>}
                      {d.model && (
                        <Tag color="blue" style={{ fontSize: 11 }}>
                          {d.model}
                        </Tag>
                      )}
                    </Space>
                    <DraftBody d={d} />
                    <DraftResearchBlock d={d} />
                    <DraftQuestionsBlock d={d} onDone={() => { load(); onChanged() }} />
                    {d.reason && (
                      <Text type="secondary" italic style={{ fontSize: 12 }}>
                        Lý do: {d.reason}
                      </Text>
                    )}
                    {d.error && (
                      <div>
                        <Text type="danger" style={{ fontSize: 12 }}>
                          {d.error}
                        </Text>
                      </div>
                    )}
                    <div>
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        {fmtDateTime(d.created_at)}
                        {d.posted_ref ? ` · ref ${d.posted_ref}` : ''}
                      </Text>
                    </div>
                  </div>
                  {d.verify_post_id && d.verify_code ? (
                    // Post exists on Moltbook — only verification is missing.
                    // Deliberately NOT offering "duyệt" here: that would duplicate.
                    <Space direction="vertical">
                      <Tooltip title="Bài đã có trên Moltbook, chỉ giải lại challenge — không đăng lại">
                        <Button size="small" type="primary" icon={<SyncOutlined />} onClick={() => retryVerify(d)}>
                          Xác minh lại
                        </Button>
                      </Tooltip>
                      <Text type="secondary" style={{ fontSize: 10 }}>
                        challenge hết hạn ~5 phút
                      </Text>
                    </Space>
                  ) : d.status === 'pending' ? (
                    <Space direction="vertical">
                      <Popconfirm
                        title="Đăng lên Moltbook?"
                        description="Nội dung này sẽ được đăng công khai dưới danh nghĩa agent của bạn."
                        okText="Đăng"
                        cancelText="Huỷ"
                        onConfirm={() => approve(d)}
                      >
                        <Button type="primary" size="small" icon={<SendOutlined />}>
                          Duyệt & đăng
                        </Button>
                      </Popconfirm>
                      <Button size="small" danger onClick={() => reject(d)}>
                        Từ chối
                      </Button>
                    </Space>
                  ) : null}
                </Flex>
              </Card>
            ))}
          </Space>
        )}
      </Spin>
    </div>
  )
}

function DraftBody({ d }: { d: Draft }) {
  if (d.kind === 'post') {
    return (
      <div style={{ marginBottom: 4 }}>
        <div style={{ fontWeight: 600 }}>
          {d.title} <Text type="secondary" style={{ fontWeight: 400, fontSize: 12 }}>→ m/{d.submolt}</Text>
        </div>
        {d.content && <Text type="secondary">{d.content}</Text>}
      </div>
    )
  }
  if (d.kind === 'comment') {
    return (
      <div style={{ marginBottom: 4 }}>
        {d.target_title && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            trả lời: “{d.target_title}”
          </Text>
        )}
        <div>{d.content}</div>
      </div>
    )
  }
  if (d.kind === 'vote') {
    return (
      <div style={{ marginBottom: 4 }}>
        {d.vote_dir === 'down' ? '⬇ downvote' : '⬆ upvote'} “{d.target_title || d.target_post_id}”
      </div>
    )
  }
  if (d.kind === 'submolt') {
    return (
      <div style={{ marginBottom: 4 }}>
        Tạo m/{d.submolt} {d.title ? `(${d.title})` : ''}
        {d.content && <div><Text type="secondary">{d.content}</Text></div>}
      </div>
    )
  }
  return (
    <div style={{ marginBottom: 4 }}>
      {KIND_LABEL[d.kind] || d.kind}: {d.target_name}
    </div>
  )
}

function StatusTag({ status }: { status: string }) {
  const map: Record<string, { color: string; label: string }> = {
    pending: { color: 'gold', label: 'Chờ duyệt' },
    needs_input: { color: 'purple', label: 'Cần trả lời' },
    posted: { color: 'green', label: 'Đã đăng' },
    rejected: { color: 'default', label: 'Từ chối' },
    error: { color: 'red', label: 'Lỗi' },
  }
  const m = map[status] || { color: 'default', label: status }
  return <Tag color={m.color}>{m.label}</Tag>
}

/// The research trail a draft was grounded in: confidence, findings, sources.
function DraftResearchBlock({ d }: { d: Draft }) {
  const [open, setOpen] = useState(false)
  const r = draftResearch(d)
  if (!r) return null
  const okSteps = r.runs?.filter((x) => x.ok) || []
  const confColor = r.confidence >= 70 ? 'green' : r.confidence >= 40 ? 'gold' : 'red'
  return (
    <div
      style={{
        margin: '6px 0',
        padding: '8px 10px',
        borderRadius: 8,
        background: 'rgba(125, 90, 255, 0.06)',
        border: '1px solid rgba(125, 90, 255, 0.25)',
      }}
    >
      <Space size={6} wrap>
        <ExperimentOutlined style={{ color: '#9a7bff' }} />
        <Text style={{ fontSize: 12 }} strong>
          Nghiên cứu
        </Text>
        <Tag color={confColor} style={{ fontSize: 10 }}>
          tin cậy {r.confidence}%
        </Tag>
        <Text type="secondary" style={{ fontSize: 11 }}>
          {okSteps.length}/{r.runs?.length || 0} bước · {(r.workflows || []).join(' · ')}
        </Text>
        <Button size="small" type="link" style={{ fontSize: 11, padding: 0 }} onClick={() => setOpen(!open)}>
          {open ? 'thu gọn' : 'chi tiết'}
        </Button>
      </Space>
      {open && (
        <div style={{ marginTop: 6 }}>
          {r.findings && (
            <Paragraph style={{ fontSize: 12, whiteSpace: 'pre-wrap', marginBottom: 6 }}>{r.findings}</Paragraph>
          )}
          {(r.key_facts || []).length > 0 && (
            <ul style={{ margin: '0 0 6px 16px', padding: 0 }}>
              {r.key_facts.map((f, i) => (
                <li key={i}>
                  <Text style={{ fontSize: 12 }}>{f}</Text>
                </li>
              ))}
            </ul>
          )}
          <Space direction="vertical" size={2} style={{ width: '100%' }}>
            {(r.runs || []).map((run, i) => (
              <Text key={i} type={run.ok ? 'secondary' : 'danger'} style={{ fontSize: 11 }}>
                {run.ok ? '✓' : '✗'} {run.step} · {run.ms}ms{run.ok ? '' : ` — ${run.output}`}
              </Text>
            ))}
          </Space>
        </div>
      )}
    </div>
  )
}

/// Open questions parked on a needs_input draft + the answer box.
function DraftQuestionsBlock({ d, onDone }: { d: Draft; onDone: () => void }) {
  const [answer, setAnswer] = useState('')
  const [busy, setBusy] = useState(false)
  const questions = draftQuestions(d)
  if (d.status !== 'needs_input' || questions.length === 0) {
    // Answered earlier — show the trail.
    if (d.answer) {
      return (
        <Text type="secondary" italic style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
          Bạn đã trả lời: {d.answer}
        </Text>
      )
    }
    return null
  }
  const submit = async (skip: boolean) => {
    setBusy(true)
    const hide = message.loading(skip ? 'Đang bỏ qua câu hỏi…' : 'Đang soạn lại theo câu trả lời…', 0)
    try {
      await api.answerDraft(d.id, skip ? '' : answer.trim())
      hide()
      message.success(skip ? 'Đã chuyển về hàng chờ duyệt (giữ nội dung).' : 'Đã soạn lại — kiểm tra rồi duyệt.')
      onDone()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div
      style={{
        margin: '6px 0',
        padding: '8px 10px',
        borderRadius: 8,
        background: 'rgba(255, 173, 20, 0.08)',
        border: '1px solid rgba(255, 173, 20, 0.35)',
      }}
    >
      <Space size={6} style={{ marginBottom: 4 }}>
        <QuestionCircleOutlined style={{ color: '#faad14' }} />
        <Text strong style={{ fontSize: 12 }}>
          AI chưa chắc chắn — cần bạn trả lời trước khi đăng
        </Text>
      </Space>
      <ul style={{ margin: '0 0 8px 16px', padding: 0 }}>
        {questions.map((q, i) => (
          <li key={i}>
            <Text style={{ fontSize: 13 }}>{q}</Text>
          </li>
        ))}
      </ul>
      <Input.TextArea
        rows={2}
        value={answer}
        onChange={(e) => setAnswer(e.target.value)}
        placeholder="Trả lời / bổ sung thông tin, chỉ đạo cho AI…"
        style={{ marginBottom: 6 }}
      />
      <Space wrap>
        <Button type="primary" size="small" loading={busy} disabled={!answer.trim()} onClick={() => submit(false)}>
          Trả lời & soạn lại
        </Button>
        <Tooltip title="Bỏ qua câu hỏi, giữ nguyên nội dung hiện tại và chuyển về hàng chờ duyệt">
          <Button size="small" loading={busy} onClick={() => submit(true)}>
            Bỏ qua, giữ nội dung
          </Button>
        </Tooltip>
      </Space>
    </div>
  )
}

// ---------------------------------------------------------------- Trending

/// What the whole agent internet is talking about → a dated wiki briefing.
function TrendingTab({ active, account }: { active: boolean; account: Account | null }) {
  const [digests, setDigests] = useState<TrendingDigest[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [last, setLast] = useState<TrendingRun | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setDigests(await api.digests())
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    if (active) load()
  }, [active, load])

  const run = async () => {
    setBusy(true)
    const hide = message.loading('Đang quét feed và tổng hợp xu hướng…', 0)
    try {
      const r = await api.runTrending(true)
      hide()
      setLast(r)
      if (r.ok) message.success(r.note || 'Đã tổng hợp.')
      else message.warning(r.reason || 'Không tổng hợp được.')
      load()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <Flex justify="space-between" align="center" wrap gap={8} style={{ marginBottom: 12 }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Quét feed hot + rising + top → gom thành chủ đề → ghi tài liệu vào wiki (mỗi ngày một bản)
        </Text>
        <Space>
          <Button type="primary" icon={<FireOutlined />} loading={busy} onClick={run} disabled={!account?.connected}>
            Tổng hợp xu hướng
          </Button>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={load}>
            Làm mới
          </Button>
        </Space>
      </Flex>

      {last?.ok && last.topic_list && last.topic_list.length > 0 && (
        <Card size="small" style={{ marginBottom: 12 }} title={`Vừa tổng hợp — ${last.day}`}>
          {last.summary && <Paragraph style={{ marginBottom: 10 }}>{last.summary}</Paragraph>}
          <Space direction="vertical" size={8} style={{ width: '100%' }}>
            {last.topic_list.map((t, i) => (
              <div key={i}>
                <Space size={6} wrap>
                  <Text strong>{t.name}</Text>
                  {t.relevant && <Tag color="gold">khớp chủ đề của bạn</Tag>}
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {t.post_count} bài
                  </Text>
                </Space>
                {t.takeaway && (
                  <div>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      → {t.takeaway}
                    </Text>
                  </div>
                )}
              </div>
            ))}
          </Space>
          {last.wiki_path && (
            <div style={{ marginTop: 10 }}>
              <Text type="secondary" style={{ fontSize: 11 }}>
                📄 {last.wiki_path}
              </Text>
            </div>
          )}
        </Card>
      )}

      <Spin spinning={loading}>
        {digests.length === 0 ? (
          <Empty description="Chưa có bản tổng hợp nào — bấm “Tổng hợp xu hướng”" />
        ) : (
          <Space direction="vertical" size={10} style={{ width: '100%' }}>
            {digests.map((d) => (
              <Card key={d.day} size="small">
                <Space size={6} wrap style={{ marginBottom: 4 }}>
                  <Tag color="volcano">{d.day}</Tag>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {d.topic_count} chủ đề · {d.post_count} bài
                  </Text>
                  {d.runs > 1 && (
                    <Tooltip title={`Đã tổng hợp lại ${d.runs} lần trong ngày`}>
                      <Tag style={{ fontSize: 10 }}>×{d.runs}</Tag>
                    </Tooltip>
                  )}
                </Space>
                {d.summary && (
                  <Paragraph type="secondary" style={{ marginBottom: 6 }} ellipsis={{ rows: 3, expandable: true, symbol: 'thêm' }}>
                    {d.summary}
                  </Paragraph>
                )}
                {d.topics.length > 0 && (
                  <Space size={4} wrap>
                    {d.topics.map((t, i) => (
                      <Tag key={i}>{t}</Tag>
                    ))}
                  </Space>
                )}
                {d.wiki_path && (
                  <div style={{ marginTop: 6 }}>
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      📄 {d.wiki_path} · cập nhật {fmtRelative(d.updated_at)}
                    </Text>
                  </div>
                )}
              </Card>
            ))}
          </Space>
        )}
      </Spin>
    </div>
  )
}

// ------------------------------------------------------- My posts / feedback

/// The feedback loop: our posts → what other agents said → the wiki doc.
function MyPostsTab({ active, account, onChanged }: { active: boolean; account: Account | null; onChanged: () => void }) {
  const [posts, setPosts] = useState<TrackedPost[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setPosts(await api.tracked())
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    if (active) load()
  }, [active, load])

  const run = async (postId?: string) => {
    setBusy(postId || 'all')
    const hide = message.loading(postId ? 'Đang cập nhật doc…' : 'Đang thu thập phản hồi…', 0)
    try {
      const r = await api.harvest(postId)
      hide()
      if (r.ok) message.success(r.note || 'Xong.')
      else message.warning(r.reason || 'Không chạy được.')
      load()
      onChanged()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    } finally {
      setBusy(null)
    }
  }

  const staleCount = posts.filter((p) => p.doc_is_stale).length

  return (
    <div>
      <Flex justify="space-between" align="center" wrap gap={8} style={{ marginBottom: 12 }}>
        <Space wrap>
          <Text type="secondary" style={{ fontSize: 12 }}>
            Bài molty đã đăng · thu thập bình luận của agent khác → tổng hợp → cập nhật doc wiki
          </Text>
          {staleCount > 0 && <Tag color="gold">{staleCount} doc cần cập nhật</Tag>}
        </Space>
        <Space>
          <Button
            type="primary"
            icon={<SyncOutlined />}
            loading={busy === 'all'}
            onClick={() => run()}
            disabled={!account?.connected}
          >
            Thu thập phản hồi
          </Button>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={load}>
            Làm mới
          </Button>
        </Space>
      </Flex>

      <Spin spinning={loading}>
        {posts.length === 0 ? (
          <Empty description="Chưa có bài nào được theo dõi — bài molty đăng sẽ tự vào đây" />
        ) : (
          <Space direction="vertical" size={12} style={{ width: '100%' }}>
            {posts.map((p) => (
              <Card key={p.post_id} size="small">
                <Flex justify="space-between" align="flex-start" gap={12} wrap>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Space size={6} wrap style={{ marginBottom: 4 }}>
                      {p.submolt && <Tag color="volcano">m/{p.submolt}</Tag>}
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        ⬆ {p.last_score} · 💬 {p.last_comment_count}
                      </Text>
                      {p.doc_is_stale ? (
                        <Tag color="gold">doc cần cập nhật</Tag>
                      ) : p.last_synced_at ? (
                        <Tag color="green">doc đã cập nhật</Tag>
                      ) : null}
                    </Space>
                    <div style={{ fontWeight: 600, marginBottom: 4 }}>{p.title || p.post_id}</div>

                    {p.synthesis ? (
                      <Paragraph
                        type="secondary"
                        style={{ marginBottom: 6, whiteSpace: 'pre-wrap' }}
                        ellipsis={{ rows: 4, expandable: true, symbol: 'xem thêm' }}
                      >
                        {p.synthesis}
                      </Paragraph>
                    ) : (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {p.last_comment_count > 0 ? 'Chưa tổng hợp — bấm “Cập nhật doc”.' : 'Chưa có bình luận nào.'}
                      </Text>
                    )}

                    {p.last_error && (
                      <div>
                        <Text type="danger" style={{ fontSize: 12 }}>
                          {p.last_error}
                        </Text>
                      </div>
                    )}

                    <div style={{ marginTop: 4 }}>
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        đã kiểm tra {p.checks} lần
                        {p.last_checked_at ? ` · lần cuối ${fmtRelative(p.last_checked_at)}` : ''}
                        {p.last_synced_at ? ` · doc cập nhật ${fmtRelative(p.last_synced_at)}` : ''}
                        {p.wiki_path ? ` · ${p.wiki_path}` : ''}
                      </Text>
                    </div>
                  </div>

                  <Space direction="vertical">
                    <Button
                      size="small"
                      type={p.doc_is_stale ? 'primary' : 'default'}
                      icon={<SyncOutlined />}
                      loading={busy === p.post_id}
                      onClick={() => run(p.post_id)}
                      disabled={!account?.connected}
                    >
                      Cập nhật doc
                    </Button>
                    <Popconfirm
                      title="Bỏ theo dõi bài này?"
                      description="Chỉ ngừng thu thập phản hồi; bài trên Moltbook và doc wiki không bị xoá."
                      okText="Bỏ theo dõi"
                      cancelText="Huỷ"
                      onConfirm={async () => {
                        try {
                          await api.untrackPost(p.post_id)
                          load()
                        } catch (e) {
                          message.error((e as Error).message)
                        }
                      }}
                    >
                      <Button size="small" type="text" danger>
                        Bỏ theo dõi
                      </Button>
                    </Popconfirm>
                  </Space>
                </Flex>
              </Card>
            ))}
          </Space>
        )}
      </Spin>
    </div>
  )
}

// ---------------------------------------------------------------- Activity

function ActivityTab({ active }: { active: boolean }) {
  const [items, setItems] = useState<Activity[]>([])
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setItems(await api.activity(100))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (active) load()
  }, [active, load])

  const colorOf = (k: string) =>
    k === 'error' ? 'red' : k === 'post' || k === 'comment' || k === 'vote' ? 'green' : k === 'heartbeat' ? 'blue' : 'gray'

  return (
    <div>
      <Flex justify="flex-end" style={{ marginBottom: 12 }}>
        <Button icon={<ReloadOutlined />} loading={loading} onClick={load}>
          Làm mới
        </Button>
      </Flex>
      {items.length === 0 ? (
        <Empty description="Chưa có hoạt động" />
      ) : (
        <Timeline
          items={items.map((it) => ({
            color: colorOf(it.kind),
            children: (
              <div>
                <Space size={6}>
                  <Tag>{it.kind}</Tag>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {fmtRelative(it.created_at)}
                  </Text>
                </Space>
                <div>{it.text}</div>
              </div>
            ),
          }))}
        />
      )}
    </div>
  )
}

// ---------------------------------------------------------------- Research

const FLOW_LABEL: Record<string, string> = {
  comment: 'Bình luận',
  post: 'Đăng bài',
  both: 'Cả hai',
}

const FLOW_OPTS = [
  { value: 'both', label: 'Cả hai luồng' },
  { value: 'comment', label: 'Bình luận bài viết' },
  { value: 'post', label: 'Đăng bài mới' },
]

function stepLabel(s: ResearchStep): string {
  if (s.kind === 'app') return `${s.app}/${s.tool}`
  if (s.kind === 'daemon') return `${s.server}/${s.tool}`
  return s.tool
}

/// Nghiên cứu: workflows chọn tool MCP chạy trước khi bình luận / đăng bài,
/// tổng hợp tri thức, và hỏi lại người dùng khi chưa chắc chắn.
function ResearchTab({
  active,
  account,
  onChanged,
}: {
  active: boolean
  account: Account | null
  onChanged: () => void
}) {
  const [workflows, setWorkflows] = useState<ResearchWorkflow[]>([])
  const [catalog, setCatalog] = useState<ToolCatalog | null>(null)
  const [loading, setLoading] = useState(false)
  const [editing, setEditing] = useState<ResearchWorkflow | 'new' | null>(null)
  const [testing, setTesting] = useState<ResearchWorkflow | null>(null)
  const [aiDesc, setAiDesc] = useState('')
  const [aiFlow, setAiFlow] = useState('both')
  const [aiBusy, setAiBusy] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setWorkflows(await api.workflows())
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    if (active) {
      load()
      // The catalog needs the daemon + peer apps — fetch lazily, best-effort.
      api.researchTools().then(setCatalog).catch(() => setCatalog(null))
    }
  }, [active, load])

  const toggle = async (w: ResearchWorkflow, enabled: boolean) => {
    try {
      await api.patchWorkflow(w.id, { enabled })
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const remove = async (w: ResearchWorkflow) => {
    try {
      await api.deleteWorkflow(w.id)
      message.success(`Đã xoá workflow “${w.name}”.`)
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const aiBuild = async () => {
    if (!aiDesc.trim()) return
    setAiBusy(true)
    const hide = message.loading('AI đang đọc catalog công cụ và soạn workflow…', 0)
    try {
      const r = await api.aiBuildWorkflow(aiDesc.trim(), aiFlow)
      hide()
      message.success(`Đã tạo workflow “${r.workflow.name}” (${(r.workflow.steps_parsed || JSON.parse(r.workflow.steps) || []).length} bước).`)
      setAiDesc('')
      load()
    } catch (e) {
      hide()
      message.error((e as Error).message)
    } finally {
      setAiBusy(false)
    }
  }

  const nApps = catalog?.apps.length || 0
  const nDaemon = catalog?.daemon.length || 0

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Alert
        type="info"
        showIcon
        icon={<ExperimentOutlined />}
        message="Nghiên cứu trước khi đăng / bình luận"
        description="Trước khi soạn nội dung, molty chạy các workflow bên dưới (song song) để tổng hợp thông tin qua công cụ MCP — trí nhớ, wiki, Moltbook, các Space App khác… — rồi viết dựa trên kết quả. Nếu độ tin cậy thấp hơn ngưỡng, bài sẽ nằm ở Hàng chờ duyệt với trạng thái “Cần trả lời” để hỏi bạn trước."
      />

      {account && <ResearchSettingsCard account={account} onChanged={onChanged} />}

      <Card
        title="AI tạo workflow"
        size="small"
        extra={
          <Text type="secondary" style={{ fontSize: 11 }}>
            catalog: {catalog ? `${catalog.builtin.length} builtin · ${nApps} app · ${nDaemon} MCP server` : 'đang tải…'}
          </Text>
        }
      >
        <Space.Compact style={{ width: '100%' }}>
          <Select value={aiFlow} onChange={setAiFlow} style={{ width: 170 }} options={FLOW_OPTS} />
          <Input
            value={aiDesc}
            onChange={(e) => setAiDesc(e.target.value)}
            onPressEnter={aiBuild}
            placeholder="VD: tra wiki nội bộ + tìm bài liên quan trên Moltbook, rồi lấy tin tức mới về chủ đề"
          />
          <Button type="primary" icon={<RobotOutlined />} loading={aiBusy} onClick={aiBuild} disabled={!aiDesc.trim()}>
            AI tạo
          </Button>
        </Space.Compact>
        <Text type="secondary" style={{ fontSize: 12 }}>
          AI đọc catalog công cụ thật (builtin + Space App + MCP server của daemon), tự chọn bước và lưu thành workflow — bạn xem lại và sửa được.
        </Text>
      </Card>

      <Flex justify="space-between" align="center" wrap gap={8}>
        <Text strong>Workflow ({workflows.length})</Text>
        <Space>
          <Button icon={<EditOutlined />} onClick={() => setEditing('new')}>
            Tạo thủ công
          </Button>
          <Button icon={<ReloadOutlined />} loading={loading} onClick={load}>
            Làm mới
          </Button>
        </Space>
      </Flex>

      <Spin spinning={loading}>
        {workflows.length === 0 ? (
          <Empty description="Chưa có workflow — dùng “AI tạo” hoặc “Tạo thủ công”" />
        ) : (
          <Space direction="vertical" size={10} style={{ width: '100%' }}>
            {workflows.map((w) => {
              const steps: ResearchStep[] = w.steps_parsed || []
              return (
                <Card key={w.id} size="small" style={{ opacity: w.enabled ? 1 : 0.55 }}>
                  <Flex justify="space-between" align="flex-start" gap={12} wrap>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <Space size={6} wrap style={{ marginBottom: 6 }}>
                        <Switch size="small" checked={w.enabled} onChange={(v) => toggle(w, v)} />
                        <Text strong>{w.name}</Text>
                        <Tag color={w.flow === 'both' ? 'geekblue' : w.flow === 'post' ? 'volcano' : 'cyan'}>
                          {FLOW_LABEL[w.flow] || w.flow}
                        </Tag>
                        {w.builtin && <Tag style={{ fontSize: 10 }}>mặc định</Tag>}
                      </Space>
                      <Space size={4} wrap style={{ marginBottom: 4 }}>
                        {steps.map((s, i) => (
                          <Tooltip key={i} title={JSON.stringify(s.args || {}, null, 1)}>
                            <Tag
                              color={s.kind === 'builtin' ? 'default' : s.kind === 'app' ? 'blue' : 'purple'}
                              style={{ fontSize: 11, margin: 0 }}
                            >
                              {i + 1}. {stepLabel(s)}
                            </Tag>
                          </Tooltip>
                        ))}
                      </Space>
                      {w.extract_prompt && (
                        <div>
                          <Text type="secondary" italic style={{ fontSize: 12 }}>
                            Extract thêm: {w.extract_prompt}
                          </Text>
                        </div>
                      )}
                    </div>
                    <Space direction="vertical" size={4}>
                      <Button size="small" icon={<PlayCircleOutlined />} onClick={() => setTesting(w)}>
                        Chạy thử
                      </Button>
                      <Button size="small" icon={<EditOutlined />} onClick={() => setEditing(w)}>
                        Sửa
                      </Button>
                      <Popconfirm title={`Xoá workflow “${w.name}”?`} okText="Xoá" cancelText="Huỷ" onConfirm={() => remove(w)}>
                        <Button size="small" type="text" danger>
                          Xoá
                        </Button>
                      </Popconfirm>
                    </Space>
                  </Flex>
                </Card>
              )
            })}
          </Space>
        )}
      </Spin>

      {catalog && <ToolCatalogCard catalog={catalog} />}

      <WorkflowEditorModal
        editing={editing}
        catalog={catalog}
        onClose={() => setEditing(null)}
        onSaved={() => {
          setEditing(null)
          load()
        }}
      />
      <ResearchTestModal workflow={testing} onClose={() => setTesting(null)} />
    </Space>
  )
}

/// Cài đặt nghiên cứu: bật/tắt, ngưỡng hỏi lại, prompt extract chung.
function ResearchSettingsCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [extract, setExtract] = useState(account.research_extract_prompt)
  const patch = async (p: Partial<Account>) => {
    try {
      await api.putSettings(p)
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  return (
    <Card title="Cài đặt nghiên cứu" size="small">
      <Form layout="vertical">
        <Space size={24} wrap>
          <Form.Item label="Bật nghiên cứu" style={{ marginBottom: 8 }}>
            <Switch checked={account.research_enabled} onChange={(v) => patch({ research_enabled: v })} />
          </Form.Item>
          <Form.Item
            label="Cả khi bấm “AI soạn”"
            style={{ marginBottom: 8 }}
            tooltip="Nút 'Nháp trả lời (AI)' và 'AI soạn giúp' cũng nghiên cứu trước khi viết."
          >
            <Switch
              checked={account.research_on_compose}
              disabled={!account.research_enabled}
              onChange={(v) => patch({ research_on_compose: v })}
            />
          </Form.Item>
          <Form.Item
            label="Ngưỡng hỏi lại (%)"
            style={{ marginBottom: 8 }}
            tooltip="Độ tin cậy dưới ngưỡng này → draft chuyển sang 'Cần trả lời' và hỏi bạn trước khi đăng. 0 = không bao giờ hỏi."
          >
            <InputNumber
              min={0}
              max={100}
              value={account.research_ask_threshold}
              onChange={(v) => patch({ research_ask_threshold: v ?? 60 })}
            />
          </Form.Item>
          <Form.Item
            label="Số mục nghiên cứu mỗi heartbeat"
            style={{ marginBottom: 8 }}
            tooltip="Giới hạn số bình luận/bài được nghiên cứu mỗi vòng heartbeat (tiết kiệm token)."
          >
            <InputNumber
              min={0}
              max={10}
              value={account.research_max_per_tick}
              onChange={(v) => patch({ research_max_per_tick: v ?? 3 })}
            />
          </Form.Item>
        </Space>
        <Form.Item
          label="Yêu cầu extract thêm (áp dụng cho mọi workflow)"
          help="Được đưa vào bước tổng hợp — VD: “luôn trích số liệu cụ thể và nguồn”, “chú ý các ý kiến phản biện”."
        >
          <Input.TextArea
            rows={2}
            value={extract}
            onChange={(e) => setExtract(e.target.value)}
            onBlur={() => extract.trim() !== account.research_extract_prompt && patch({ research_extract_prompt: extract.trim() })}
            placeholder="VD: luôn trích số liệu cụ thể kèm nguồn; ghi rõ điểm còn tranh cãi"
          />
        </Form.Item>
      </Form>
    </Card>
  )
}

/// Danh mục công cụ MCP khả dụng cho workflow.
function ToolCatalogCard({ catalog }: { catalog: ToolCatalog }) {
  const [open, setOpen] = useState(false)
  return (
    <Card
      size="small"
      title="Công cụ khả dụng"
      extra={
        <Button size="small" type="link" onClick={() => setOpen(!open)}>
          {open ? 'thu gọn' : 'xem tất cả'}
        </Button>
      }
    >
      {!open ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {catalog.builtin.length} builtin (trí nhớ / wiki / Moltbook) · {catalog.apps.length} Space App có MCP (
          {catalog.apps.map((a) => a.id).join(', ') || 'không có'}) · {catalog.daemon.length} MCP server trên daemon
        </Text>
      ) : (
        <Space direction="vertical" size={10} style={{ width: '100%' }}>
          <div>
            <Text strong style={{ fontSize: 12 }}>
              Builtin
            </Text>
            {catalog.builtin.map((t) => (
              <div key={t.tool}>
                <Text code style={{ fontSize: 11 }}>
                  {t.tool}
                </Text>{' '}
                <Text type="secondary" style={{ fontSize: 11 }}>
                  {t.description}
                </Text>
              </div>
            ))}
          </div>
          {catalog.apps.map((a) => (
            <div key={a.id}>
              <Text strong style={{ fontSize: 12 }}>
                App: {a.name} ({a.id})
              </Text>
              {a.tools.map((t) => (
                <div key={t.tool}>
                  <Text code style={{ fontSize: 11 }}>
                    {t.tool}
                  </Text>{' '}
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {t.description}
                  </Text>
                </div>
              ))}
            </div>
          ))}
          {catalog.daemon.map((s) => (
            <div key={s.name}>
              <Text strong style={{ fontSize: 12 }}>
                MCP server: {s.name}
              </Text>
              {s.tools.length === 0 ? (
                <div>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    (không liệt kê tool — vẫn gọi được nếu biết tên)
                  </Text>
                </div>
              ) : (
                s.tools.map((t) => (
                  <div key={t.tool}>
                    <Text code style={{ fontSize: 11 }}>
                      {t.tool}
                    </Text>{' '}
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      {t.description}
                    </Text>
                  </div>
                ))
              )}
            </div>
          ))}
        </Space>
      )}
    </Card>
  )
}

/// Tạo/sửa workflow: form + steps JSON (bấm tool trong catalog để thêm bước).
function WorkflowEditorModal({
  editing,
  catalog,
  onClose,
  onSaved,
}: {
  editing: ResearchWorkflow | 'new' | null
  catalog: ToolCatalog | null
  onClose: () => void
  onSaved: () => void
}) {
  const isNew = editing === 'new'
  const w = isNew ? null : (editing as ResearchWorkflow | null)
  const [name, setName] = useState('')
  const [flow, setFlow] = useState('both')
  const [extract, setExtract] = useState('')
  const [stepsText, setStepsText] = useState('[]')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (editing === null) return
    if (isNew) {
      setName('')
      setFlow('both')
      setExtract('')
      setStepsText(JSON.stringify([{ kind: 'builtin', tool: 'wiki_context', args: { query: '{{topic}}' } }], null, 2))
    } else if (w) {
      setName(w.name)
      setFlow(w.flow)
      setExtract(w.extract_prompt)
      const parsed = w.steps_parsed || []
      setStepsText(JSON.stringify(parsed.length ? parsed : JSON.parse(w.steps || '[]'), null, 2))
    }
  }, [editing, isNew, w])

  const parsedSteps = useMemo<ResearchStep[] | null>(() => {
    try {
      const v = JSON.parse(stepsText)
      return Array.isArray(v) ? (v as ResearchStep[]) : null
    } catch {
      return null
    }
  }, [stepsText])

  const appendStep = (s: ResearchStep) => {
    const cur = parsedSteps || []
    setStepsText(JSON.stringify([...cur, s], null, 2))
  }

  const save = async () => {
    if (!name.trim() || !parsedSteps || parsedSteps.length === 0) return
    setBusy(true)
    try {
      if (isNew) {
        await api.createWorkflow({ name: name.trim(), flow, steps: parsedSteps, extract_prompt: extract })
        message.success('Đã tạo workflow.')
      } else if (w) {
        await api.patchWorkflow(w.id, { name: name.trim(), flow, steps: parsedSteps, extract_prompt: extract })
        message.success('Đã lưu workflow.')
      }
      onSaved()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      title={isNew ? 'Tạo workflow nghiên cứu' : `Sửa: ${w?.name || ''}`}
      open={editing !== null}
      onCancel={onClose}
      onOk={save}
      okText="Lưu"
      confirmLoading={busy}
      width={720}
      okButtonProps={{ disabled: !name.trim() || !parsedSteps || parsedSteps.length === 0 }}
    >
      <Space direction="vertical" style={{ width: '100%' }} size={10}>
        <Space.Compact style={{ width: '100%' }}>
          <Select value={flow} onChange={setFlow} style={{ width: 180 }} options={FLOW_OPTS} />
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Tên workflow" />
        </Space.Compact>
        <div>
          <Text strong style={{ fontSize: 12 }}>
            Các bước (JSON){' '}
            {parsedSteps === null && (
              <Text type="danger" style={{ fontSize: 12 }}>
                — JSON không hợp lệ
              </Text>
            )}
          </Text>
          <Input.TextArea
            rows={10}
            value={stepsText}
            onChange={(e) => setStepsText(e.target.value)}
            style={{ fontFamily: 'monospace', fontSize: 12 }}
          />
          <Text type="secondary" style={{ fontSize: 11 }}>
            Placeholder trong args: {'{{topic}}'} {'{{title}}'} {'{{content}}'} {'{{post_id}}'} — và {'{{tên_save_as}}'} của bước trước.
          </Text>
        </div>
        {catalog && (
          <div>
            <Text strong style={{ fontSize: 12 }}>
              Bấm để thêm bước:
            </Text>
            <div style={{ maxHeight: 140, overflow: 'auto', marginTop: 4 }}>
              <Space size={4} wrap>
                {catalog.builtin.map((t) => (
                  <Tag
                    key={t.tool}
                    style={{ cursor: 'pointer', fontSize: 11 }}
                    onClick={() => appendStep({ kind: 'builtin', tool: t.tool, args: (t.args || {}) as Record<string, unknown> })}
                  >
                    + {t.tool}
                  </Tag>
                ))}
                {catalog.apps.flatMap((a) =>
                  a.tools.map((t) => (
                    <Tag
                      key={`${a.id}/${t.tool}`}
                      color="blue"
                      style={{ cursor: 'pointer', fontSize: 11 }}
                      onClick={() => appendStep({ kind: 'app', app: a.id, tool: t.tool, args: { query: '{{topic}}' } })}
                    >
                      + {a.id}/{t.tool}
                    </Tag>
                  )),
                )}
                {catalog.daemon.flatMap((s) =>
                  s.tools.map((t) => (
                    <Tag
                      key={`${s.name}/${t.tool}`}
                      color="purple"
                      style={{ cursor: 'pointer', fontSize: 11 }}
                      onClick={() => appendStep({ kind: 'daemon', server: s.name, tool: t.tool, args: { query: '{{topic}}' } })}
                    >
                      + {s.name}/{t.tool}
                    </Tag>
                  )),
                )}
              </Space>
            </div>
          </div>
        )}
        <div>
          <Text strong style={{ fontSize: 12 }}>
            Yêu cầu extract thêm (riêng workflow này)
          </Text>
          <Input.TextArea
            rows={2}
            value={extract}
            onChange={(e) => setExtract(e.target.value)}
            placeholder="VD: chỉ lấy thông tin có nguồn; ưu tiên dữ liệu 30 ngày gần nhất"
          />
        </div>
      </Space>
    </Modal>
  )
}

/// Chạy thử nghiên cứu với một chủ đề và xem bundle kết quả.
function ResearchTestModal({ workflow, onClose }: { workflow: ResearchWorkflow | null; onClose: () => void }) {
  const [topic, setTopic] = useState('')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<ResearchRunResult | null>(null)

  useEffect(() => {
    if (workflow) {
      setTopic('')
      setResult(null)
    }
  }, [workflow])

  const run = async () => {
    if (!workflow || !topic.trim()) return
    setBusy(true)
    setResult(null)
    try {
      const flow = workflow.flow === 'both' ? 'post' : workflow.flow
      setResult(await api.researchRun({ flow, topic: topic.trim() }))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const b = result?.bundle
  return (
    <Modal
      title={workflow ? `Chạy thử nghiên cứu (luồng ${FLOW_LABEL[workflow.flow] || workflow.flow})` : ''}
      open={!!workflow}
      onCancel={onClose}
      footer={null}
      width={720}
    >
      <Space direction="vertical" style={{ width: '100%' }} size={10}>
        <Alert
          type="info"
          showIcon={false}
          message={
            <Text style={{ fontSize: 12 }}>
              Chạy thử dùng MỌI workflow đang bật khớp luồng này (giống lúc chạy thật — các workflow chạy song song).
            </Text>
          }
        />
        <Space.Compact style={{ width: '100%' }}>
          <Input
            value={topic}
            onChange={(e) => setTopic(e.target.value)}
            onPressEnter={run}
            placeholder="Chủ đề để nghiên cứu thử…"
          />
          <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={run} disabled={!topic.trim()}>
            Chạy
          </Button>
        </Space.Compact>
        {result && !result.ok && <Alert type="error" message={result.error} />}
        {b && (
          <div>
            <Space size={6} wrap style={{ marginBottom: 6 }}>
              <Tag color={b.confidence >= 70 ? 'green' : b.confidence >= 40 ? 'gold' : 'red'}>tin cậy {b.confidence}%</Tag>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {b.runs.filter((r) => r.ok).length}/{b.runs.length} bước ok · {b.workflows.join(' · ')} · {b.model}
              </Text>
            </Space>
            <Paragraph style={{ whiteSpace: 'pre-wrap', fontSize: 13 }}>{b.findings}</Paragraph>
            {b.key_facts.length > 0 && (
              <ul style={{ margin: '0 0 8px 16px' }}>
                {b.key_facts.map((f, i) => (
                  <li key={i}>
                    <Text style={{ fontSize: 12 }}>{f}</Text>
                  </li>
                ))}
              </ul>
            )}
            {(result.gated_questions || []).length > 0 && (
              <Alert
                type="warning"
                showIcon
                icon={<QuestionCircleOutlined />}
                message="Với kết quả này, AI sẽ hỏi bạn trước khi đăng:"
                description={
                  <ul style={{ margin: '4px 0 0 16px' }}>
                    {(result.gated_questions || []).map((q, i) => (
                      <li key={i}>{q}</li>
                    ))}
                  </ul>
                }
              />
            )}
            <div style={{ marginTop: 8 }}>
              {b.runs.map((r, i) => (
                <div key={i}>
                  <Text type={r.ok ? 'secondary' : 'danger'} style={{ fontSize: 11 }}>
                    {r.ok ? '✓' : '✗'} [{r.workflow}] {r.step} · {r.ms}ms{r.ok ? '' : ` — ${r.output}`}
                  </Text>
                </div>
              ))}
            </div>
          </div>
        )}
      </Space>
    </Modal>
  )
}

// ---------------------------------------------------------------- Settings

function SettingsTab({ account, onChanged }: { account: Account | null; onChanged: () => void }) {
  if (!account) return <Spin />
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <ConnectionCard account={account} onChanged={onChanged} />
      <ParticipationCard account={account} onChanged={onChanged} />
      <TopicsCard account={account} onChanged={onChanged} />
      <IntegrationsCard account={account} onChanged={onChanged} />
      <ProfileCard account={account} onChanged={onChanged} />
      <Alert
        type="warning"
        showIcon
        message="Bảo mật khoá API"
        description="API key chỉ lưu trong DB cục bộ của app và chỉ được gửi tới base URL đã cấu hình (mặc định www.moltbook.com). Không dán key lên host khác. Nên xoay khoá định kỳ."
      />
    </Space>
  )
}

interface ClaimState {
  url: string
  code: string
  raw: unknown
}

function ClaimBlock({ claim, onRecover, recovering }: { claim: ClaimState; onRecover: () => void; recovering: boolean }) {
  const hasUrl = !!claim.url
  return (
    <Alert
      type={hasUrl ? 'success' : 'warning'}
      message={hasUrl ? 'Có link claim' : 'Chưa lấy được link claim tự động'}
      description={
        <Space direction="vertical" style={{ width: '100%' }}>
          {hasUrl ? (
            <>
              <Text>Mở link này và xác nhận bằng tài khoản X để kích hoạt agent:</Text>
              <a href={claim.url} target="_blank" rel="noreferrer" style={{ wordBreak: 'break-all' }}>
                {claim.url}
              </a>
              {claim.code && (
                <Text code copyable>
                  {claim.code}
                </Text>
              )}
            </>
          ) : (
            <>
              <Text>
                Không dò được link trong phản hồi của Moltbook. Xem phản hồi thô bên dưới (link claim thường nằm trong
                đó), hoặc bấm “Lấy lại link claim”.
              </Text>
              <pre
                style={{
                  maxHeight: 220,
                  overflow: 'auto',
                  background: 'rgba(0,0,0,.35)',
                  padding: 10,
                  borderRadius: 6,
                  fontSize: 12,
                  margin: 0,
                }}
              >
                {JSON.stringify(claim.raw ?? {}, null, 2)}
              </pre>
            </>
          )}
          <Button size="small" icon={<ReloadOutlined />} loading={recovering} onClick={onRecover}>
            Lấy lại link claim
          </Button>
        </Space>
      }
    />
  )
}

function ConnectionCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [apiKey, setApiKey] = useState('')
  const [regName, setRegName] = useState('')
  const [regDesc, setRegDesc] = useState('')
  const [busy, setBusy] = useState(false)
  const [claimBusy, setClaimBusy] = useState(false)
  const [claim, setClaim] = useState<ClaimState | null>(null)

  const connect = async () => {
    if (!apiKey.trim()) return
    setBusy(true)
    try {
      await api.connect(apiKey.trim())
      message.success('Đã kết nối agent.')
      setApiKey('')
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  const register = async () => {
    if (!regName.trim()) return
    setBusy(true)
    try {
      const r = await api.register(regName.trim(), regDesc.trim())
      setClaim({ url: r.claim_url, code: r.verification_code, raw: r.raw })
      if (r.claim_url) message.success('Đã đăng ký — mở link claim để kích hoạt.')
      else message.warning('Đã đăng ký, nhưng chưa dò được link claim — xem phản hồi thô.')
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  const recoverClaim = async () => {
    setClaimBusy(true)
    try {
      const r = await api.claimInfo()
      if (r.claim_url) {
        setClaim({ url: r.claim_url, code: '', raw: r.status ?? r.last_register_response })
        message.success('Đã lấy được link claim.')
      } else if (r.claimed) {
        setClaim(null)
        message.success('Agent đã được claim rồi ✔')
      } else {
        setClaim({ url: '', code: '', raw: { status: r.status, me: r.me, last_register_response: r.last_register_response } })
        message.warning('Vẫn chưa tìm thấy link claim — xem phản hồi thô.')
      }
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setClaimBusy(false)
    }
  }
  const disconnect = async () => {
    try {
      await api.disconnect()
      setClaim(null)
      message.success('Đã ngắt kết nối.')
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <Card title="Kết nối Moltbook" size="small">
      {account.connected ? (
        <Space direction="vertical" style={{ width: '100%' }}>
          <Text>
            Đang kết nối với <b>@{account.agent_name || 'molty'}</b>
            {account.claimed ? '' : ' (chưa claim)'} · base {account.base_url}
          </Text>
          <Space wrap>
            <Button
              icon={<ReloadOutlined />}
              onClick={async () => {
                try {
                  await api.refresh()
                  message.success('Đã làm mới hồ sơ.')
                  onChanged()
                } catch (e) {
                  message.error((e as Error).message)
                }
              }}
            >
              Làm mới hồ sơ
            </Button>
            {!account.claimed && (
              <Button icon={<ReloadOutlined />} loading={claimBusy} onClick={recoverClaim}>
                Lấy link claim / kiểm tra trạng thái
              </Button>
            )}
            <Popconfirm title="Xoá API key khỏi máy?" onConfirm={disconnect} okText="Xoá" cancelText="Huỷ">
              <Button danger icon={<DisconnectOutlined />}>
                Ngắt kết nối
              </Button>
            </Popconfirm>
          </Space>
          {!account.claimed && account.claim_url && !claim && (
            <ClaimBlock
              claim={{ url: account.claim_url, code: account.verification_code, raw: null }}
              onRecover={recoverClaim}
              recovering={claimBusy}
            />
          )}
          {claim && <ClaimBlock claim={claim} onRecover={recoverClaim} recovering={claimBusy} />}
        </Space>
      ) : (
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <div>
            <Text strong>Đã có API key?</Text>
            <Space.Compact style={{ width: '100%', marginTop: 6 }}>
              <Input.Password
                placeholder="Dán Moltbook API key"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                onPressEnter={connect}
              />
              <Button type="primary" loading={busy} onClick={connect}>
                Kết nối
              </Button>
            </Space.Compact>
          </div>
          <div>
            <Text strong>Hoặc đăng ký molty mới</Text>
            <Space direction="vertical" style={{ width: '100%', marginTop: 6 }}>
              <Input placeholder="Tên agent" value={regName} onChange={(e) => setRegName(e.target.value)} />
              <Input placeholder="Mô tả ngắn (agent này làm gì)" value={regDesc} onChange={(e) => setRegDesc(e.target.value)} />
              <Button loading={busy} onClick={register} disabled={!regName.trim()}>
                Đăng ký
              </Button>
            </Space>
          </div>
          {claim && <ClaimBlock claim={claim} onRecover={recoverClaim} recovering={claimBusy} />}
        </Space>
      )}
    </Card>
  )
}

function ParticipationCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [voice, setVoice] = useState(account.persona_voice)
  const [submolt, setSubmolt] = useState(account.default_submolt)
  const [minutes, setMinutes] = useState(account.heartbeat_minutes)
  const [engage, setEngage] = useState(account.engage_limit)
  const [hb, setHb] = useState(account.heartbeat_enabled)
  const [harvest, setHarvest] = useState(account.harvest_enabled)
  const [trendingDaily, setTrendingDaily] = useState(account.trending_daily)
  const [busy, setBusy] = useState(false)

  const save = async () => {
    setBusy(true)
    try {
      await api.putSettings({
        persona_voice: voice,
        default_submolt: submolt,
        heartbeat_minutes: minutes,
        engage_limit: engage,
        heartbeat_enabled: hb,
        harvest_enabled: harvest,
        trending_daily: trendingDaily,
      })
      message.success('Đã lưu cài đặt tham gia.')
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title="Tham gia" size="small">
      <Form layout="vertical">
        <Form.Item label="Heartbeat tự động" help="Định kỳ đọc feed rồi soạn/đăng theo chế độ ở trên (Quan sát/Nháp/Tự động).">
          <Space>
            <Switch checked={hb} onChange={setHb} />
            <Text>mỗi</Text>
            <InputNumber min={5} max={1440} value={minutes} onChange={(v) => setMinutes(v || 60)} addonAfter="phút" />
          </Space>
        </Form.Item>
        <Form.Item label="Số tương tác mỗi vòng" help="Tối đa bao nhiêu bình luận engine soạn mỗi lần chạy.">
          <InputNumber min={0} max={10} value={engage} onChange={(v) => setEngage(v ?? 2)} />
        </Form.Item>
        <Form.Item label="Submolt mặc định khi đăng bài">
          <Input addonBefore="m/" value={submolt} onChange={(e) => setSubmolt(e.target.value)} />
        </Form.Item>
        <Form.Item
          label="Tự thu thập phản hồi & cập nhật doc"
          help="Mỗi lần heartbeat, đọc bình luận của các agent khác trên bài của bạn, tổng hợp lại và ghi lại doc wiki. Bỏ qua bài không có bình luận mới."
        >
          <Switch checked={harvest} onChange={setHarvest} />
        </Form.Item>
        <Form.Item
          label="Tự tổng hợp xu hướng mỗi ngày"
          help="Mỗi ngày một lần, quét feed và ghi tài liệu xu hướng vào wiki. Tốn thêm một lượt gọi LLM nên mặc định tắt."
        >
          <Switch checked={trendingDaily} onChange={setTrendingDaily} />
        </Form.Item>
        <Form.Item label="Giọng / persona của molty" help="Được đưa vào prompt khi engine quyết định tương tác và soạn nội dung.">
          <Input.TextArea rows={4} value={voice} onChange={(e) => setVoice(e.target.value)} placeholder="Mặc định: một agent tò mò, điềm đạm, tham gia có chọn lọc…" />
        </Form.Item>
        <Button type="primary" loading={busy} onClick={save}>
          Lưu
        </Button>
      </Form>
    </Card>
  )
}

const TOPIC_KINDS: Array<{ value: Topic['kind']; label: string; hint: string }> = [
  { value: 'engage', label: 'Tương tác', hint: 'Chủ đề molty tìm & phản hồi trên feed' },
  { value: 'post', label: 'Đăng / hỏi', hint: 'Điều bạn muốn molty đăng bài hoặc hỏi' },
  { value: 'both', label: 'Cả hai', hint: 'Dùng cho cả tương tác lẫn đăng bài' },
]

/// Steer the molty: which subjects it engages with, and what you want it to
/// post/ask about on Moltbook.
function TopicsCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [topics, setTopics] = useState<Topic[]>([])
  const [loading, setLoading] = useState(false)
  const [text, setText] = useState('')
  const [kind, setKind] = useState<Topic['kind']>('both')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setTopics(await api.topics())
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    load()
  }, [load])

  const add = async () => {
    if (!text.trim()) return
    try {
      await api.addTopic(text.trim(), kind)
      setText('')
      message.success('Đã thêm chủ đề.')
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const patch = async (id: number, p: Partial<Pick<Topic, 'text' | 'kind' | 'enabled'>>) => {
    try {
      await api.patchTopic(id, p)
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const remove = async (id: number) => {
    try {
      await api.deleteTopic(id)
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const setMode = async (mode: string) => {
    try {
      await api.putSettings({ topic_mode: mode as Account['topic_mode'] })
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const engageCount = topics.filter((t) => t.enabled && (t.kind === 'engage' || t.kind === 'both')).length
  const focusBroken = account.topic_mode === 'focus' && engageCount === 0

  return (
    <Card title="Chủ đề & định hướng" size="small">
      <Space direction="vertical" size={12} style={{ width: '100%' }}>
        <div>
          <Text strong>Phạm vi tương tác</Text>
          <div style={{ marginTop: 6 }}>
            <Segmented
              value={account.topic_mode}
              onChange={(v) => setMode(String(v))}
              options={[
                { label: 'Toàn bộ feed', value: 'all' },
                { label: 'Chỉ chủ đề đã chọn', value: 'focus' },
              ]}
            />
          </div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {account.topic_mode === 'focus'
              ? 'Molty chỉ tương tác với bài liên quan các chủ đề “Tương tác” bên dưới, bỏ qua phần còn lại.'
              : 'Molty đọc cả feed; chủ đề bên dưới chỉ dùng để ưu tiên.'}
          </Text>
        </div>

        {focusBroken && (
          <Alert
            type="warning"
            showIcon
            message="Chưa có chủ đề “Tương tác” nào"
            description="Đang ở chế độ “Chỉ chủ đề đã chọn” nhưng danh sách trống — molty sẽ KHÔNG tương tác gì. Thêm một chủ đề, hoặc chuyển về “Toàn bộ feed”."
          />
        )}

        <div>
          <Text strong>Danh sách chủ đề</Text>
          <Space.Compact style={{ width: '100%', marginTop: 6 }}>
            <Select
              value={kind}
              onChange={setKind}
              style={{ width: 130 }}
              options={TOPIC_KINDS.map((k) => ({ value: k.value, label: k.label }))}
            />
            <Input
              value={text}
              onChange={(e) => setText(e.target.value)}
              onPressEnter={add}
              placeholder={
                kind === 'post'
                  ? 'VD: Hỏi các molty khác cách xử lý rate limit'
                  : 'VD: trí nhớ của agent, MCP, xây dựng công khai'
              }
            />
            <Button type="primary" onClick={add} disabled={!text.trim()}>
              Thêm
            </Button>
          </Space.Compact>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {TOPIC_KINDS.find((k) => k.value === kind)?.hint}
          </Text>
        </div>

        <Spin spinning={loading}>
          {topics.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="Chưa có chủ đề — molty tự do chọn theo persona"
            />
          ) : (
            <Space direction="vertical" size={6} style={{ width: '100%' }}>
              {topics.map((t) => (
                <Flex key={t.id} align="center" gap={8} style={{ opacity: t.enabled ? 1 : 0.45 }}>
                  <Switch size="small" checked={t.enabled} onChange={(v) => patch(t.id, { enabled: v })} />
                  <Select
                    size="small"
                    value={t.kind}
                    style={{ width: 110 }}
                    onChange={(v) => patch(t.id, { kind: v })}
                    options={TOPIC_KINDS.map((k) => ({ value: k.value, label: k.label }))}
                  />
                  <Text style={{ flex: 1, minWidth: 0 }} ellipsis={{ tooltip: t.text }}>
                    {t.text}
                  </Text>
                  {t.used_at ? (
                    <Tooltip title={`Đã dùng để đăng bài ${fmtRelative(t.used_at)}`}>
                      <Tag style={{ fontSize: 10 }}>đã dùng</Tag>
                    </Tooltip>
                  ) : null}
                  <Popconfirm title="Xoá chủ đề này?" okText="Xoá" cancelText="Huỷ" onConfirm={() => remove(t.id)}>
                    <Button size="small" type="text" danger>
                      Xoá
                    </Button>
                  </Popconfirm>
                </Flex>
              ))}
            </Space>
          )}
        </Spin>

        <Text type="secondary" style={{ fontSize: 12 }}>
          Cũng thêm/sửa được qua MCP: <Text code>moltbook_add_topic</Text>, <Text code>moltbook_list_topics</Text>,{' '}
          <Text code>moltbook_set_topic_mode</Text>.
        </Text>
      </Space>
    </Card>
  )
}

/// knowledge = trí nhớ của molty · wiki = kho thông tin chung của Sếp.
function IntegrationsCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [status, setStatus] = useState<Integrations | null>(null)
  const [space, setSpace] = useState(account.knowledge_space)
  const [busy, setBusy] = useState(false)
  const [q, setQ] = useState('')
  const [recall, setRecall] = useState<RecallResult | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await api.integrations())
    } catch {
      setStatus(null)
    }
  }, [])
  useEffect(() => {
    loadStatus()
  }, [loadStatus])

  const patch = async (p: Partial<Account>) => {
    try {
      await api.putSettings(p)
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const doRecall = async () => {
    if (!q.trim()) return
    setBusy(true)
    try {
      setRecall(await api.memoryRecall(q.trim()))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card
      title="Kết nối SenClaw — Trí nhớ & Kho thông tin"
      size="small"
      extra={
        <Space size={4}>
          <Tag color={status?.knowledge.available ? 'green' : 'default'}>
            trí nhớ {status?.knowledge.available ? 'ok' : 'chưa sẵn sàng'}
          </Tag>
          <Tag color={status?.wiki.available ? 'green' : 'default'}>
            wiki {status?.wiki.available ? 'ok' : 'chưa sẵn sàng'}
          </Tag>
          <Button size="small" type="text" icon={<ReloadOutlined />} onClick={loadStatus} />
        </Space>
      }
    >
      <Form layout="vertical">
        <Form.Item
          label="Trí nhớ (knowledge space)"
          help="Molty tự nhớ mọi thứ nó ĐÃ ĐĂNG, và nhớ lại trước khi soạn — để không lặp lại và giữ nhất quán giữa các lần heartbeat."
        >
          <Space wrap>
            <Switch checked={account.memory_enabled} onChange={(v) => patch({ memory_enabled: v })} />
            <Input
              addonBefore="space"
              style={{ width: 240 }}
              value={space}
              onChange={(e) => setSpace(e.target.value)}
              onBlur={() => space.trim() !== account.knowledge_space && patch({ knowledge_space: space.trim() })}
            />
          </Space>
        </Form.Item>

        <Form.Item
          label="Kho thông tin (wiki)"
          help="Trước khi soạn bài/trả lời, molty tra wiki của Sếp và nói DỰA TRÊN tài liệu thật thay vì bịa."
        >
          <Switch checked={account.wiki_enabled} onChange={(v) => patch({ wiki_enabled: v })} />
        </Form.Item>

        <Form.Item label="Tự lưu bài đã đăng vào wiki" help="Mỗi bài molty đăng lên Moltbook sẽ được lưu một bản vào moltbook/posts/ trong wiki.">
          <Switch
            checked={account.wiki_archive}
            disabled={!account.wiki_enabled}
            onChange={(v) => patch({ wiki_archive: v })}
          />
        </Form.Item>

        <Form.Item label="Thử trí nhớ" help="Hỏi xem molty còn nhớ gì (ví dụ: 'tôi đã nói gì về submolt existential').">
          <Space.Compact style={{ width: '100%' }}>
            <Input value={q} onChange={(e) => setQ(e.target.value)} onPressEnter={doRecall} placeholder="molty nhớ gì về…" />
            <Button loading={busy} onClick={doRecall} disabled={!q.trim()}>
              Nhớ lại
            </Button>
          </Space.Compact>
        </Form.Item>
      </Form>

      {recall && (
        <Alert
          type={recall.grounded ? 'success' : 'info'}
          message={recall.grounded ? `Trí nhớ (${recall.space})` : 'Chưa có gì trong trí nhớ về việc này'}
          description={
            recall.grounded ? (
              <Space direction="vertical" style={{ width: '100%' }}>
                <Text>{recall.answer}</Text>
                {recall.hits.length > 0 && (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {recall.hits.length} mẩu ký ức: {recall.hits.map((h) => h.name).filter(Boolean).join(' · ')}
                  </Text>
                )}
              </Space>
            ) : (
              'Molty sẽ nhớ dần sau mỗi bài/bình luận được duyệt đăng.'
            )
          }
        />
      )}
    </Card>
  )
}

/// Pick WHICH SenClaw LLM profile this app composes with. Local to Moltbook —
/// it never changes the daemon's active model (that would hijack every other
/// app/chat). "" = follow whatever the daemon has active.
function ProfileCard({ account, onChanged }: { account: Account; onChanged: () => void }) {
  const [models, setModels] = useState<ModelConfig[]>([])
  const [activeId, setActiveId] = useState<string | undefined>(undefined)
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const r = await api.models()
      setModels(r.configs || [])
      setActiveId(r.activeId || undefined)
    } catch {
      /* daemon may be offline — non-fatal */
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    load()
  }, [load])

  const nameOf = (m: ModelConfig) => m.label?.trim() || m.modelName || m.id
  const activeLabel = useMemo(() => {
    const m = models.find((x) => x.id === activeId)
    return m ? nameOf(m) : null
  }, [models, activeId])

  const options = useMemo(
    () => [
      {
        value: '',
        label: `Theo daemon${activeLabel ? ` — hiện tại: ${activeLabel}` : ''}`,
      },
      ...models.map((m) => ({
        value: m.id,
        label: (
          <span>
            {nameOf(m)}
            <Text type="secondary" style={{ fontSize: 12 }}>
              {' '}
              · {m.modelName || m.id}
              {m.provider || m.adapt ? ` · ${m.provider || m.adapt}` : ''}
            </Text>
            {m.id === activeId ? (
              <Tag color="blue" style={{ marginLeft: 6, fontSize: 10 }}>
                active
              </Tag>
            ) : null}
          </span>
        ),
      })),
    ],
    [models, activeId, activeLabel],
  )

  // The stored value may be an id OR a label (both resolve daemon-side); map a
  // stored label back onto its id so the Select highlights the right row.
  const selected = useMemo(() => {
    const v = account.llm_profile || ''
    if (!v) return ''
    if (models.some((m) => m.id === v)) return v
    const byLabel = models.find((m) => (m.label || '').toLowerCase() === v.toLowerCase())
    return byLabel ? byLabel.id : v
  }, [account.llm_profile, models])

  const choose = async (id: string) => {
    try {
      await api.putSettings({ llm_profile: id })
      message.success(id ? `Moltbook sẽ soạn bằng profile: ${nameOf(models.find((m) => m.id === id) || { id })}` : 'Đã chuyển về model active của daemon.')
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <Card title="Profile LLM của Moltbook (dùng để soạn nội dung)" size="small">
      <Space direction="vertical" style={{ width: '100%' }}>
        <Space wrap>
          <Select
            style={{ minWidth: 340 }}
            loading={loading}
            value={selected}
            options={options}
            placeholder="Chọn profile"
            onChange={choose}
            notFoundContent={loading ? <Spin size="small" /> : 'Daemon chưa cấu hình profile nào'}
          />
          <Button icon={<ReloadOutlined />} onClick={load} />
        </Space>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Chỉ áp dụng cho Moltbook — <b>không</b> đổi model active của daemon. Profile lấy từ SenClaw
          Settings → Models (tên profile là nhãn, ví dụ <Text code>MoltClaw</Text>).
        </Text>
        {account.llm_profile && !models.some((m) => m.id === selected) && (
          <Alert
            type="warning"
            showIcon
            message={`Không tìm thấy profile "${account.llm_profile}"`}
            description="Profile có thể đã bị đổi tên hoặc xoá. Chọn lại một profile bên dưới, nếu không việc soạn nội dung sẽ báo lỗi."
          />
        )}
      </Space>
    </Card>
  )
}
