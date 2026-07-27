import { useCallback, useEffect, useState } from 'react'
import {
  App,
  AutoComplete,
  Button,
  Card,
  Form,
  Image,
  Input,
  Modal,
  Popconfirm,
  Radio,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
  Upload,
} from 'antd'
import type { UploadFile } from 'antd'
import { EditOutlined, PictureOutlined, PlusOutlined } from '@ant-design/icons'
import {
  ago,
  compose,
  getAccounts,
  getDrafts,
  mutate,
  platformRule,
  type Account,
  type Draft,
  type Status,
} from '../api'
import { StatusTag } from '../ui'

/** Read a File into a base64 data URL. */
function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(r.result as string)
    r.onerror = reject
    r.readAsDataURL(file)
  })
}

type Filter = 'pending' | 'sent' | 'rejected' | 'all'

export default function Drafts({ status, onChanged }: { status: Status | null; onChanged: () => void }) {
  const { message } = App.useApp()
  const [drafts, setDrafts] = useState<Draft[]>([])
  const [filter, setFilter] = useState<Filter>('pending')
  const [busy, setBusy] = useState<number | null>(null)

  // Compose modal
  const [open, setOpen] = useState(false)
  const [sending, setSending] = useState(false)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [files, setFiles] = useState<UploadFile[]>([])
  const [form] = Form.useForm()
  const cPlatform: string = Form.useWatch('platform', form) ?? 'facebook'
  const cKind: 'post' | 'dm' = Form.useWatch('kind', form) ?? 'post'
  const rule = platformRule(cPlatform)
  const dmSupported = (status?.capabilities?.[cPlatform]?.dm ?? 'replay') !== 'none'

  const load = useCallback(async () => setDrafts((await getDrafts())?.drafts ?? []), [])

  useEffect(() => {
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [load])

  useEffect(() => {
    if (open) {
      getAccounts().then((r) => setAccounts(r?.accounts ?? []))
      setFiles([])
    }
  }, [open])

  // A platform with no DM can't compose a DM — force back to post.
  useEffect(() => {
    if (cKind === 'dm' && !dmSupported) form.setFieldValue('kind', 'post')
    // Trim files over the platform's cap.
    if (files.length > rule.mediaMax) setFiles((f) => f.slice(0, rule.mediaMax))
  }, [cPlatform, cKind, dmSupported, rule.mediaMax, form, files.length])

  const act = async (id: number, kind: 'approve' | 'reject') => {
    setBusy(id)
    const r = await mutate(`/api/drafts/${id}/${kind}`, 'POST')
    setBusy(null)
    if (r.ok) message.success(kind === 'approve' ? `Đã gửi nháp #${id}` : `Đã bỏ nháp #${id}`)
    else message.error(r.error ?? 'Thất bại')
    await load()
    onChanged()
  }

  const autonomy = status?.autonomy ?? 'draft'
  const platforms = status?.platforms ?? ['facebook', 'x', 'threads', 'instagram', 'tiktok', 'youtube']
  const handleOptions = accounts
    .filter((a) => a.platform === cPlatform)
    .map((a) => ({ value: a.handle, label: a.display_name ? `${a.handle} — ${a.display_name}` : a.handle }))

  const submitCompose = async (v: {
    platform: string
    handle: string
    kind: 'post' | 'dm'
    text: string
    thread_id?: string
  }) => {
    // Platform-specific guard: media required (e.g. Instagram).
    if (v.kind === 'post' && rule.mediaRequired && files.length === 0) {
      message.error(rule.mediaNote ?? `${v.platform} cần ít nhất 1 ảnh/video.`)
      return
    }
    setSending(true)
    // Gather any already-loaded data URLs, converting freshly-added files.
    const media: string[] = []
    for (const f of files.slice(0, rule.mediaMax)) {
      if (typeof f.url === 'string') media.push(f.url)
      else if (f.originFileObj) media.push(await fileToDataUrl(f.originFileObj as File))
    }
    const r = await compose({
      platform: v.platform,
      handle: v.handle.trim(),
      kind: v.kind,
      text: v.text,
      thread_id: v.thread_id?.trim(),
      media: v.kind === 'post' ? media : undefined,
    })
    setSending(false)
    if (r.ok) {
      message.success(r.data?.drafted ? 'Đã tạo nháp chờ duyệt.' : 'Đã gửi.')
      setOpen(false)
      form.resetFields(['text', 'thread_id'])
      setFiles([])
      await load()
      onChanged()
    } else message.error(r.error ?? 'Thất bại')
  }

  const count = (f: Filter) => (f === 'all' ? drafts.length : drafts.filter((d) => d.status === f).length)
  const rows = drafts.filter((d) => filter === 'all' || d.status === filter)

  const modeTag =
    autonomy === 'live' ? (
      <Tag color="orange">live — gửi ngay</Tag>
    ) : autonomy === 'observe' ? (
      <Tag color="purple">observe — chỉ đọc</Tag>
    ) : (
      <Tag color="blue">draft — tạo nháp chờ duyệt</Tag>
    )

  return (
    <Card
      size="small"
      title={
        <Space>
          Nháp & soạn mới
          {modeTag}
        </Space>
      }
      extra={
        <Button type="primary" icon={<EditOutlined />} onClick={() => setOpen(true)}>
          Soạn mới
        </Button>
      }
    >
      <Tabs
        activeKey={filter}
        onChange={(k) => setFilter(k as Filter)}
        items={(['pending', 'sent', 'rejected', 'all'] as Filter[]).map((f) => ({
          key: f,
          label: `${{ pending: 'Chờ duyệt', sent: 'Đã gửi', rejected: 'Đã bỏ', all: 'Tất cả' }[f]} (${count(f)})`,
        }))}
      />
      <Table<Draft>
        size="small"
        rowKey="id"
        dataSource={rows}
        pagination={{ pageSize: 20, hideOnSinglePage: true }}
        locale={{ emptyText: 'Không có nháp nào ở mục này.' }}
        columns={[
          { title: 'Nền tảng', dataIndex: 'platform', width: 110 },
          { title: 'Loại', dataIndex: 'kind', width: 80 },
          {
            title: 'Nội dung',
            dataIndex: 'text',
            render: (v: string, r) => (
              <>
                <div>{v}</div>
                {!!r.media?.length && (
                  <Space size={4} style={{ marginTop: 4 }}>
                    <PictureOutlined style={{ opacity: 0.6 }} />
                    <Image.PreviewGroup>
                      {r.media.map((src, i) => (
                        <Image key={i} src={src} width={40} height={40} style={{ objectFit: 'cover', borderRadius: 4 }} />
                      ))}
                    </Image.PreviewGroup>
                  </Space>
                )}
                {r.detail && (
                  <Typography.Text type="danger" style={{ fontSize: 12 }}>
                    {r.detail}
                  </Typography.Text>
                )}
                {r.ref_id && <div className="mono">ref: {r.ref_id}</div>}
              </>
            ),
          },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            width: 110,
            render: (v: string) => <StatusTag value={v} />,
          },
          {
            title: 'Tạo lúc',
            dataIndex: 'created_at',
            width: 90,
            render: (v: string) => <span className="mono">{ago(v)}</span>,
          },
          {
            title: '',
            width: 190,
            render: (_, r) =>
              r.status === 'pending' && (
                <Space>
                  <Button type="primary" size="small" loading={busy === r.id} onClick={() => act(r.id, 'approve')}>
                    Duyệt &amp; gửi
                  </Button>
                  <Popconfirm title="Bỏ nháp này?" onConfirm={() => act(r.id, 'reject')}>
                    <Button danger size="small">
                      Bỏ
                    </Button>
                  </Popconfirm>
                </Space>
              ),
          },
        ]}
      />

      <Modal
        title="Soạn bài đăng / tin nhắn"
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        okText={autonomy === 'live' ? 'Gửi ngay' : 'Tạo nháp'}
        okButtonProps={{ loading: sending, disabled: autonomy === 'observe' }}
        destroyOnClose
      >
        <Form form={form} layout="vertical" initialValues={{ platform: 'facebook', kind: 'post' }} onFinish={submitCompose}>
          <Form.Item name="kind" label="Loại">
            <Radio.Group
              optionType="button"
              buttonStyle="solid"
              options={[
                { value: 'post', label: 'Bài đăng' },
                { value: 'dm', label: 'Tin nhắn', disabled: !dmSupported },
              ]}
            />
          </Form.Item>
          <Space.Compact block>
            <Form.Item name="platform" label="Nền tảng" rules={[{ required: true }]} style={{ width: '45%' }}>
              <Select options={platforms.map((p) => ({ value: p, label: p }))} />
            </Form.Item>
            <Form.Item name="handle" label="Tài khoản" rules={[{ required: true, message: 'Chọn/nhập handle' }]} style={{ width: '55%' }}>
              <AutoComplete options={handleOptions} placeholder="@handle / tên Page" filterOption />
            </Form.Item>
          </Space.Compact>
          {cKind === 'dm' && (
            <Form.Item
              name="thread_id"
              label="Người nhận / Thread ID"
              rules={[{ required: true, message: 'Nhắn tin cần thread/người nhận' }]}
              extra="ID cuộc trò chuyện (từ Hộp thư). Nhắn tin chỉ để trả lời cuộc trò chuyện đã có."
            >
              <Input placeholder="vd: t-42 / external_id" />
            </Form.Item>
          )}
          <Form.Item
            name="text"
            label="Nội dung"
            rules={[{ required: true, message: 'Nhập nội dung' }]}
            extra={`Giới hạn ${cPlatform}: ${rule.maxChars.toLocaleString('vi')} ký tự`}
          >
            <Input.TextArea
              rows={5}
              maxLength={rule.maxChars}
              showCount
              placeholder={cKind === 'dm' ? 'Nội dung tin nhắn…' : 'Nội dung bài đăng…'}
            />
          </Form.Item>

          {cKind === 'post' && rule.mediaMax > 0 && (
            <Form.Item label={`Ảnh (tối đa ${rule.mediaMax})`} required={rule.mediaRequired}>
              <Upload
                listType="picture-card"
                accept="image/*"
                multiple
                fileList={files}
                beforeUpload={() => false /* keep local; convert on submit */}
                onChange={({ fileList }) => setFiles(fileList.slice(0, rule.mediaMax))}
                onPreview={async (f) => {
                  const url = f.url || (f.originFileObj ? await fileToDataUrl(f.originFileObj as File) : '')
                  if (url) Modal.info({ icon: null, content: <Image src={url} preview={false} />, width: 520 })
                }}
              >
                {files.length >= rule.mediaMax ? null : (
                  <div>
                    <PlusOutlined />
                    <div style={{ marginTop: 4 }}>Thêm ảnh</div>
                  </div>
                )}
              </Upload>
              {rule.mediaNote && (
                <Typography.Text type="warning" style={{ fontSize: 12 }}>
                  {rule.mediaNote}
                </Typography.Text>
              )}
            </Form.Item>
          )}

          {cKind === 'post' && cPlatform === 'facebook' && (
            <div style={{ marginBottom: 8 }}>
              {status?.fb_composer_ready ? (
                <Tag color="green">FB: đăng qua API nội bộ (đã học)</Tag>
              ) : (
                <Tag color="gold">FB: chưa học API — đăng tay 1 bài trên Facebook để app học, tạm dùng DOM</Tag>
              )}
            </div>
          )}
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {autonomy === 'live'
              ? 'Chế độ live: gửi thẳng qua API/extension (ảnh chỉ hỗ trợ ở chế độ draft).'
              : autonomy === 'observe'
                ? 'Chế độ observe: chỉ đọc — đổi ở Cài đặt để đăng/nhắn.'
                : 'Chế độ draft: tạo nháp, phải bấm Duyệt & gửi sau. Ảnh được lưu kèm nháp.'}
          </Typography.Text>
        </Form>
      </Modal>
    </Card>
  )
}
