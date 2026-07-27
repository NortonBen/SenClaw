import { useCallback, useEffect, useRef, useState } from 'react'
import { App, Button, Card, Col, Form, Input, Popconfirm, Row, Select, Space, Table, Tag } from 'antd'
import { LoginOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  ago,
  CFG_HINT,
  extLogin,
  extWhoami,
  getAccounts,
  mutate,
  profileUrl,
  type Account,
  type Status,
  type WhoAmI,
} from '../api'

export default function Accounts({ status, onChanged }: { status: Status | null; onChanged: () => void }) {
  const { message } = App.useApp()
  const [accounts, setAccounts] = useState<Account[]>([])
  const [form] = Form.useForm()
  const [saving, setSaving] = useState(false)
  const platform: string = Form.useWatch('platform', form) ?? 'facebook'

  const [detecting, setDetecting] = useState(false)
  const pollRef = useRef<number | null>(null)

  const load = useCallback(async () => setAccounts((await getAccounts())?.accounts ?? []), [])

  useEffect(() => {
    load()
  }, [load])

  // Stop any in-flight session poll when leaving the page.
  useEffect(() => () => { if (pollRef.current) clearInterval(pollRef.current) }, [])

  const extConnected = !!status?.extension_connected
  const sessionReady = (status?.extension_hosts_ready ?? []).includes(platform)

  // Copy detected identity into the form for the operator to confirm.
  const applyIdentity = (d: WhoAmI) => {
    const patch: { handle?: string; display_name?: string; official_config?: string } = {}
    if (d.handle) patch.handle = d.handle
    if (d.name) patch.display_name = d.name
    // Persist the captured web-session tokens under official_config.web_session
    // (a distinct key so it never clobbers the official {page_id, access_token}).
    if (d.web_config && Object.keys(d.web_config).length) {
      let cur: Record<string, unknown> = {}
      const raw = form.getFieldValue('official_config') as string | undefined
      if (raw?.trim()) {
        try {
          cur = JSON.parse(raw)
        } catch {
          /* keep {} */
        }
      }
      patch.official_config = JSON.stringify({ ...cur, web_session: d.web_config }, null, 2)
    }
    if (Object.keys(patch).length) form.setFieldsValue(patch)
    return !!(patch.handle || patch.display_name)
  }

  // ① Open the platform's login page in Chrome, then poll for the session so the
  // form fills in on its own once the user finishes signing in.
  const openLogin = async () => {
    const r = await extLogin(platform)
    if (!r.ok) {
      message.error(r.error ?? 'Không mở được trang đăng nhập')
      return
    }
    message.info('Đã mở tab đăng nhập trong Chrome — hãy đăng nhập, hệ thống sẽ tự nhận diện.')
    setDetecting(true)
    let tries = 0
    if (pollRef.current) clearInterval(pollRef.current)
    pollRef.current = window.setInterval(async () => {
      tries += 1
      const w = await extWhoami(platform)
      const done = !!w.data?.logged_in
      if (done || tries >= 30) {
        if (pollRef.current) clearInterval(pollRef.current)
        pollRef.current = null
        setDetecting(false)
        if (done) {
          applyIdentity(w.data!)
          message.success('Đã phát hiện đăng nhập — kiểm tra thông tin rồi bấm Lưu.')
          onChanged()
        } else {
          message.warning('Chưa phát hiện đăng nhập sau 90s. Bấm "Lấy thông tin" khi đã xong.')
        }
      }
    }, 3000)
  }

  // Human-readable summary of the web-API tokens the extension captured.
  const tokenNote = (d: WhoAmI): string => {
    if (!d.tokens) return ''
    const got = [
      d.tokens.fb_dtsg && 'fb_dtsg',
      d.tokens.lsd && 'lsd',
      d.tokens.access_token && 'access_token',
    ].filter(Boolean)
    return got.length ? ` — đã lấy token API: ${got.join(', ')}` : ' — chưa lấy được token API (mở/refresh tab Facebook rồi thử lại)'
  }

  // ② Manual re-check: pull identity now (after the user has logged in).
  const detectNow = async () => {
    setDetecting(true)
    const w = await extWhoami(platform)
    setDetecting(false)
    if (w.data?.logged_in) {
      const filled = applyIdentity(w.data)
      const base = filled ? 'Đã điền thông tin — kiểm tra rồi bấm Lưu.' : 'Đã đăng nhập; hãy nhập handle rồi Lưu.'
      message.success(base + tokenNote(w.data))
      onChanged()
    } else {
      message.warning(w.error ?? 'Chưa phát hiện phiên đăng nhập cho nền tảng này.')
    }
  }

  const save = async (v: { platform: string; handle: string; display_name?: string; official_config?: string }) => {
    let cfg: unknown = {}
    if (v.official_config?.trim()) {
      try {
        cfg = JSON.parse(v.official_config)
      } catch {
        message.error('official_config không phải JSON hợp lệ')
        return
      }
    }
    setSaving(true)
    const r = await mutate('/api/accounts', 'POST', {
      platform: v.platform,
      handle: v.handle.trim(),
      display_name: v.display_name?.trim() ?? '',
      official_config: cfg,
    })
    setSaving(false)
    if (r.ok) {
      message.success('Đã lưu tài khoản')
      form.resetFields(['handle', 'display_name', 'official_config'])
      await load()
      onChanged()
    } else message.error(r.error ?? 'Lỗi')
  }

  const remove = async (id: number) => {
    const r = await mutate(`/api/accounts/${id}`, 'DELETE')
    if (r.ok) message.success('Đã xoá')
    else message.error(r.error ?? 'Lỗi')
    await load()
    onChanged()
  }

  const platforms = status?.platforms ?? Object.keys(CFG_HINT)

  return (
    <Row gutter={[14, 14]}>
      <Col xs={24} lg={14}>
        <Card size="small" title="Danh sách">
          <Table<Account>
            size="small"
            rowKey="id"
            dataSource={accounts}
            pagination={false}
            locale={{ emptyText: 'Chưa có tài khoản nào.' }}
            columns={[
              { title: 'Nền tảng', dataIndex: 'platform', width: 96 },
              {
                title: 'Handle',
                dataIndex: 'handle',
                render: (v: string, r) => {
                  const url = profileUrl(r.platform, v)
                  return url ? (
                    <a href={url} target="_blank" rel="noreferrer noopener">
                      {v}
                    </a>
                  ) : (
                    v
                  )
                },
              },
              { title: 'Tên', dataIndex: 'display_name' },
              {
                title: 'Trạng thái',
                key: 'status',
                width: 104,
                render: (_, r) => {
                  const live = (status?.extension_hosts_ready ?? []).includes(r.platform)
                  return live ? <Tag color="green">phiên live</Tag> : <Tag>offline</Tag>
                },
              },
              {
                title: 'API / token',
                dataIndex: 'official_config',
                width: 118,
                render: (v: Record<string, unknown>) => {
                  const hasOfficial = v && Object.keys(v).some((k) => k !== 'web_session')
                  const hasWeb = !!(v && v.web_session)
                  return (
                    <Space size={4} wrap>
                      {hasOfficial ? <Tag color="green">API</Tag> : <Tag color="gold">chưa</Tag>}
                      {hasWeb && <Tag color="blue">web</Tag>}
                    </Space>
                  )
                },
              },
              {
                title: 'Kiểm tra lúc',
                dataIndex: 'updated_at',
                width: 96,
                render: (v: string) => <span className="mono">{ago(v)}</span>,
              },
              {
                title: '',
                width: 62,
                render: (_, r) => (
                  <Popconfirm title="Xoá tài khoản này?" onConfirm={() => remove(r.id)}>
                    <Button danger size="small">
                      Xoá
                    </Button>
                  </Popconfirm>
                ),
              },
            ]}
          />
        </Card>
      </Col>
      <Col xs={24} lg={10}>
        <Card size="small" title="Thêm / cập nhật tài khoản">
          <Form form={form} layout="vertical" initialValues={{ platform: 'facebook' }} onFinish={save}>
            <Form.Item name="platform" label="Nền tảng" rules={[{ required: true }]}>
              <Select options={platforms.map((p) => ({ value: p, label: p }))} />
            </Form.Item>

            <div
              style={{
                marginBottom: 18,
                padding: 12,
                borderRadius: 8,
                border: '1px solid rgba(128,128,128,.25)',
              }}
            >
              <div style={{ fontWeight: 600, marginBottom: 4 }}>Đăng nhập qua extension</div>
              <div style={{ fontSize: 12, opacity: 0.65, marginBottom: 10 }}>
                Mở trang đăng nhập <b>{platform}</b> ngay trong Chrome và đăng nhập bằng tài khoản của bạn. App
                không thấy mật khẩu — chỉ nhận diện phiên rồi điền sẵn handle/tên bên dưới.
              </div>
              <Space wrap>
                <Button icon={<LoginOutlined />} onClick={openLogin} loading={detecting} disabled={!extConnected}>
                  Mở đăng nhập
                </Button>
                <Button icon={<ReloadOutlined />} onClick={detectNow} disabled={!extConnected} loading={detecting}>
                  Lấy thông tin
                </Button>
                {sessionReady ? <Tag color="green">phiên: đã đăng nhập</Tag> : <Tag>phiên: chưa</Tag>}
              </Space>
              {!extConnected && (
                <div style={{ fontSize: 12, color: '#faad14', marginTop: 8 }}>
                  Extension chưa kết nối — cài & mở extension SenClaw Social trong Chrome trước.
                </div>
              )}
            </div>

            <Form.Item name="handle" label="Handle" rules={[{ required: true, message: 'Thiếu handle' }]}>
              <Input placeholder="@tenshop / tên Page" />
            </Form.Item>
            <Form.Item name="display_name" label="Tên hiển thị">
              <Input placeholder="tuỳ chọn" />
            </Form.Item>
            <Form.Item
              name="official_config"
              label="official_config (JSON)"
              extra={<span className="mono">{CFG_HINT[platform] ?? ''}</span>}
            >
              <Input.TextArea rows={3} placeholder="{}" />
            </Form.Item>
            <Button type="primary" htmlType="submit" loading={saving}>
              Lưu
            </Button>
            <div style={{ fontSize: 12, opacity: 0.65, marginTop: 8 }}>
              Trùng (nền tảng, handle) sẽ được cập nhật đè.
            </div>
          </Form>
        </Card>
      </Col>
    </Row>
  )
}
