import { useEffect, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Descriptions,
  Divider,
  Input,
  InputNumber,
  Modal,
  Radio,
  Space,
  Switch,
  Tag,
  Typography,
} from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import { api, fsModeLabel, type AppSettings, type FsMode, type Sandbox } from './api'

const MODES: FsMode[] = ['strict', 'allowlist', 'open']

/**
 * The read-isolation picker, shared by the per-sandbox tab and the app
 * defaults. One component so the three modes are described in the same words
 * wherever the user meets them.
 */
function FsModePicker({
  value,
  onChange,
  disabled,
}: {
  value: FsMode
  onChange: (m: FsMode) => void
  disabled?: boolean
}) {
  return (
    <Radio.Group
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as FsMode)}
    >
      <Space direction="vertical" size={6}>
        {MODES.map((m) => {
          const l = fsModeLabel(m)
          return (
            <Radio key={m} value={m}>
              <Space size={6}>
                <span>{l.title}</span>
                <Tag color={l.color}>{l.tag}</Tag>
              </Space>
              <div>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {l.detail}
                </Typography.Text>
              </div>
            </Radio>
          )
        })}
      </Space>
    </Radio.Group>
  )
}

/** Per-sandbox settings: what this one sandbox is allowed to do. */
export function SandboxSettings({
  sandbox,
  onChange,
}: {
  sandbox: Sandbox
  onChange: () => void
}) {
  const { message, modal } = AntApp.useApp()
  const [busy, setBusy] = useState(false)

  const isDocker = sandbox.backend === 'docker'

  const setMode = async (m: FsMode) => {
    setBusy(true)
    try {
      await api.setFsMode(sandbox.id, m)
      onChange()
      message.success(`Đã đổi mức cách ly đọc: ${fsModeLabel(m).title}`)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const setNetwork = (next: boolean) => {
    const apply = async () => {
      setBusy(true)
      try {
        await api.updateSandbox(sandbox.id, { network: next })
        onChange()
        message.success(next ? 'Đã bật mạng' : 'Đã tắt mạng')
      } catch (e) {
        message.error((e as Error).message)
      } finally {
        setBusy(false)
      }
    }
    // Only the loosening direction asks.
    if (!next) return void apply()
    modal.confirm({
      title: 'Bật mạng cho sandbox này?',
      content:
        'Mã trong sandbox sẽ ra được Internet — tải về được, và gửi đi được những gì nó đọc thấy.',
      okText: 'Bật mạng',
      cancelText: 'Thôi',
      onOk: apply,
    })
  }

  const setLimit = async (patch: Record<string, unknown>) => {
    setBusy(true)
    try {
      await api.updateSandbox(sandbox.id, patch)
      onChange()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={16}>
      <div>
        <Typography.Text strong>Cách ly đọc đĩa</Typography.Text>
        <div style={{ marginTop: 8 }}>
          {isDocker ? (
            <Alert
              type="success"
              showIcon
              message="Container đã cách ly toàn bộ đĩa"
              description="Sandbox docker chỉ thấy nội dung image của nó cộng các thư mục bạn gắn vào — không có đĩa máy thật để mà chặn thêm."
            />
          ) : (
            <FsModePicker value={sandbox.fsMode} onChange={setMode} disabled={busy} />
          )}
        </div>
      </div>

      <Divider style={{ margin: 0 }} />

      <Descriptions size="small" column={1} bordered>
        <Descriptions.Item label="Mạng">
          <Space size={10} wrap>
            <Switch
              checked={sandbox.network}
              loading={busy}
              onChange={setNetwork}
              checkedChildren="Bật"
              unCheckedChildren="Tắt"
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {sandbox.network
                ? 'Ra được Internet. Cần thiết để cài gói.'
                : 'Không ra được Internet — an toàn hơn.'}
              {isDocker && ' Đổi sẽ tạo lại container (file vẫn còn).'}
            </Typography.Text>
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label="CPU">
          <InputNumber
            min={0.1}
            max={32}
            step={0.5}
            value={sandbox.cpus}
            onChange={(v) => v != null && void setLimit({ cpus: v })}
          />
        </Descriptions.Item>
        <Descriptions.Item label="RAM (MB)">
          <Space size={8}>
            <InputNumber
              min={64}
              max={65536}
              step={256}
              value={sandbox.memoryMb}
              onChange={(v) => v != null && void setLimit({ memoryMb: v })}
            />
            {!isDocker && (
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Chạy trực tiếp không cưỡng chế được trần RAM — số này chỉ có tác dụng với docker.
              </Typography.Text>
            )}
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label="Hạn mỗi lần chạy">
          <Space size={8}>
            <InputNumber
              min={1}
              max={600}
              value={Math.round(sandbox.timeoutMs / 1000)}
              onChange={(v) => v != null && void setLimit({ timeoutMs: v * 1000 })}
            />
            <Typography.Text type="secondary">giây</Typography.Text>
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label="Backend">
          {sandbox.backend}
          {sandbox.image ? ` · ${sandbox.image}` : ''}
        </Descriptions.Item>
        <Descriptions.Item label="Trạng thái">
          {sandbox.status}
          {sandbox.lastError && (
            <Typography.Text type="danger"> — {sandbox.lastError}</Typography.Text>
          )}
        </Descriptions.Item>
        <Descriptions.Item label="Thư mục">
          <Typography.Text className="sbx-mono" copyable style={{ fontSize: 12 }}>
            {sandbox.workdir}
          </Typography.Text>
        </Descriptions.Item>
      </Descriptions>
    </Space>
  )
}

/** App-wide defaults: what a *new* sandbox starts with. */
export function AppSettingsModal({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  const { message } = AntApp.useApp()
  const [s, setS] = useState<AppSettings | null>(null)
  const [newPath, setNewPath] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!open) return
    api.settings().then(setS).catch((e) => message.error((e as Error).message))
  }, [open, message])

  const save = async () => {
    if (!s) return
    setBusy(true)
    try {
      setS(await api.saveSettings(s))
      message.success('Đã lưu cài đặt')
      onClose()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const addPath = () => {
    const p = newPath.trim()
    if (!s || !p) return
    if (!p.startsWith('/')) return message.warning('Đường dẫn phải là tuyệt đối (bắt đầu bằng /)')
    if (s.allowlist.includes(p)) return message.warning('Đường dẫn đã có trong danh sách')
    setS({ ...s, allowlist: [...s.allowlist, p] })
    setNewPath('')
  }

  return (
    <Modal
      open={open}
      title="Cài đặt mặc định"
      okText="Lưu"
      cancelText="Thôi"
      confirmLoading={busy}
      onOk={() => void save()}
      onCancel={onClose}
      width={620}
    >
      {s && (
        <Space direction="vertical" style={{ width: '100%' }} size={16}>
          <Alert
            type="info"
            showIcon
            message="Áp dụng cho sandbox tạo MỚI"
            description="Sandbox đang có giữ nguyên cài đặt của nó — đổi từng cái trong tab Cài đặt của sandbox đó."
          />

          <div>
            <Typography.Text strong>Cách ly đọc đĩa mặc định</Typography.Text>
            <div style={{ marginTop: 8 }}>
              <FsModePicker
                value={s.defaultFsMode}
                onChange={(m) => setS({ ...s, defaultFsMode: m })}
              />
            </div>
          </div>

          <div>
            <Typography.Text strong>Thư mục cho phép đọc</Typography.Text>
            <div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Chỉ có tác dụng ở chế độ "Cách ly + danh sách cho phép". Sandbox đọc được
                các thư mục này mà không cần gắn từng cái.
              </Typography.Text>
            </div>
            <Space.Compact style={{ width: '100%', marginTop: 8 }}>
              <Input
                className="sbx-mono"
                placeholder="/Users/ban/du-an"
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                onPressEnter={addPath}
              />
              <Button icon={<PlusOutlined />} onClick={addPath}>
                Thêm
              </Button>
            </Space.Compact>
            <div style={{ marginTop: 8 }}>
              {s.allowlist.length === 0 ? (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  Chưa có thư mục nào.
                </Typography.Text>
              ) : (
                <Space direction="vertical" size={4} style={{ width: '100%' }}>
                  {s.allowlist.map((p) => (
                    <Space key={p} size={6}>
                      <Typography.Text className="sbx-mono" style={{ fontSize: 12 }}>
                        {p}
                      </Typography.Text>
                      <Button
                        size="small"
                        type="text"
                        danger
                        icon={<DeleteOutlined />}
                        onClick={() =>
                          setS({ ...s, allowlist: s.allowlist.filter((x) => x !== p) })
                        }
                      />
                    </Space>
                  ))}
                </Space>
              )}
            </div>
          </div>

          <Divider style={{ margin: 0 }} />

          <Space wrap size={20}>
            <Space size={8}>
              <Switch
                checked={s.defaultNetwork}
                onChange={(v) => setS({ ...s, defaultNetwork: v })}
              />
              <Typography.Text>Mạng bật sẵn</Typography.Text>
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">CPU</Typography.Text>
              <InputNumber
                min={0.1}
                max={32}
                step={0.5}
                value={s.defaultCpus}
                onChange={(v) => v != null && setS({ ...s, defaultCpus: v })}
              />
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">RAM (MB)</Typography.Text>
              <InputNumber
                min={64}
                max={65536}
                step={256}
                value={s.defaultMemoryMb}
                onChange={(v) => v != null && setS({ ...s, defaultMemoryMb: v })}
              />
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">Hạn (giây)</Typography.Text>
              <InputNumber
                min={1}
                max={600}
                value={Math.round(s.defaultTimeoutMs / 1000)}
                onChange={(v) => v != null && setS({ ...s, defaultTimeoutMs: v * 1000 })}
              />
            </Space>
          </Space>
        </Space>
      )}
    </Modal>
  )
}
