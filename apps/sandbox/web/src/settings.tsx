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
import { useT } from './i18n'

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
  const t = useT()
  return (
    <Radio.Group
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as FsMode)}
    >
      <Space direction="vertical" size={6}>
        {MODES.map((m) => {
          const l = fsModeLabel(m, t)
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

/** Which ports are open, and in which direction. */
function PortsPanel({ sandbox, onChange }: { sandbox: Sandbox; onChange: () => void }) {
  const { message } = AntApp.useApp()
  const t = useT()
  const [listen, setListen] = useState(sandbox.ports.listen.join(', '))
  const [connect, setConnect] = useState(sandbox.ports.connect.join(', '))
  const [busy, setBusy] = useState(false)

  // Re-seed when a different sandbox is selected; a stale field would otherwise
  // apply one sandbox's ports to another.
  useEffect(() => {
    setListen(sandbox.ports.listen.join(', '))
    setConnect(sandbox.ports.connect.join(', '))
  }, [sandbox.id, sandbox.ports])

  const parse = (v: string): number[] | null => {
    const parts = v.split(',').map((x) => x.trim()).filter(Boolean)
    const out: number[] = []
    for (const p of parts) {
      const n = Number(p)
      if (!Number.isInteger(n) || n < 1 || n > 65535) return null
      out.push(n)
    }
    return out
  }

  const apply = async () => {
    const l = parse(listen)
    const c = parse(connect)
    if (l === null || c === null) return message.warning(t.portsInvalid)
    setBusy(true)
    try {
      const r = await api.setPorts(sandbox.id, l, c)
      onChange()
      message.success(t.portsSaved)
      // The backend note is the honest part: on docker and Linux, opening a
      // port grants a network. Shown as a warning, not hidden.
      if (r.note) message.warning(r.note, 8)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={10}>
      <Typography.Text strong>{t.ports}</Typography.Text>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        {t.portsBody}
      </Typography.Text>
      <Space wrap align="end" size={10}>
        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {t.listenPorts}
          </Typography.Text>
          <Input
            className="sbx-mono"
            style={{ width: 200 }}
            placeholder={t.portsPlaceholder}
            value={listen}
            onChange={(e) => setListen(e.target.value)}
          />
        </div>
        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {t.connectPorts}
          </Typography.Text>
          <Input
            className="sbx-mono"
            style={{ width: 200 }}
            placeholder="443"
            value={connect}
            onChange={(e) => setConnect(e.target.value)}
          />
        </div>
        <Button type="primary" loading={busy} onClick={() => void apply()}>
          {t.portsSave}
        </Button>
      </Space>
      <Space wrap size={6}>
        {sandbox.ports.listen.length === 0 && sandbox.ports.connect.length === 0 ? (
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {t.portsNone}
          </Typography.Text>
        ) : (
          <>
            {sandbox.ports.listen.map((p) => (
              <Tag key={`l${p}`} color="green">
                {t.reachableAt(p)}
              </Tag>
            ))}
            {sandbox.ports.connect.map((p) => (
              <Tag key={`c${p}`} color="blue">
                → :{p}
              </Tag>
            ))}
          </>
        )}
      </Space>
    </Space>
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
  const t = useT()
  const [busy, setBusy] = useState(false)

  const isDocker = sandbox.backend === 'docker'

  const setMode = async (m: FsMode) => {
    setBusy(true)
    try {
      await api.setFsMode(sandbox.id, m)
      onChange()
      message.success(t.isolationChanged(fsModeLabel(m, t).title))
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
        message.success(next ? t.networkEnabled : t.networkDisabled)
      } catch (e) {
        message.error((e as Error).message)
      } finally {
        setBusy(false)
      }
    }
    // Only the loosening direction asks.
    if (!next) return void apply()
    modal.confirm({
      title: t.enableNetworkTitle,
      content:
        t.enableNetworkBody,
      okText: t.enableNetwork,
      cancelText: t.cancel,
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
        <Typography.Text strong>{t.readIsolation}</Typography.Text>
        <div style={{ marginTop: 8 }}>
          {isDocker ? (
            <Alert
              type="success"
              showIcon
              message={t.dockerAlreadyIsolated}
              description={t.dockerAlreadyIsolatedBody}
            />
          ) : (
            <FsModePicker value={sandbox.fsMode} onChange={setMode} disabled={busy} />
          )}
        </div>
      </div>

      <Divider style={{ margin: 0 }} />

      <PortsPanel sandbox={sandbox} onChange={onChange} />

      <Divider style={{ margin: 0 }} />

      <Descriptions size="small" column={1} bordered>
        <Descriptions.Item label={t.network}>
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
                ? t.networkOnHint
                : t.networkOffHint}
              {isDocker && t.dockerRecreates}
            </Typography.Text>
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label={t.cpu}>
          <InputNumber
            min={0.1}
            max={32}
            step={0.5}
            value={sandbox.cpus}
            onChange={(v) => v != null && void setLimit({ cpus: v })}
          />
        </Descriptions.Item>
        <Descriptions.Item label={`${t.ram} (MB)`}>
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
                {t.ramNoteDirect}
              </Typography.Text>
            )}
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label={t.runDeadline}>
          <Space size={8}>
            <InputNumber
              min={1}
              max={600}
              value={Math.round(sandbox.timeoutMs / 1000)}
              onChange={(v) => v != null && void setLimit({ timeoutMs: v * 1000 })}
            />
            <Typography.Text type="secondary">{t.seconds}</Typography.Text>
          </Space>
        </Descriptions.Item>
        <Descriptions.Item label={t.backend}>
          {sandbox.backend}
          {sandbox.image ? ` · ${sandbox.image}` : ''}
        </Descriptions.Item>
        <Descriptions.Item label={t.status}>
          {sandbox.status}
          {sandbox.lastError && (
            <Typography.Text type="danger"> — {sandbox.lastError}</Typography.Text>
          )}
        </Descriptions.Item>
        <Descriptions.Item label={t.directory}>
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
  const t = useT()
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
      message.success(t.settingsSaved)
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
    if (!p.startsWith('/')) return message.warning(t.needAbsolutePath)
    if (s.allowlist.includes(p)) return message.warning(t.pathAlreadyListed)
    setS({ ...s, allowlist: [...s.allowlist, p] })
    setNewPath('')
  }

  return (
    <Modal
      open={open}
      title={t.defaultsTitle}
      okText={t.save}
      cancelText={t.cancel}
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
            message={t.defaultsScope}
            description={t.defaultsScopeBody}
          />

          <div>
            <Typography.Text strong>{t.defaultReadIsolation}</Typography.Text>
            <div style={{ marginTop: 8 }}>
              <FsModePicker
                value={s.defaultFsMode}
                onChange={(m) => setS({ ...s, defaultFsMode: m })}
              />
            </div>
          </div>

          <div>
            <Typography.Text strong>{t.allowlistFolders}</Typography.Text>
            <div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {t.allowlistFoldersBody}
              </Typography.Text>
            </div>
            <Space.Compact style={{ width: '100%', marginTop: 8 }}>
              <Input
                className="sbx-mono"
                placeholder={t.mountPathPlaceholder}
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                onPressEnter={addPath}
              />
              <Button icon={<PlusOutlined />} onClick={addPath}>
                {t.add}
              </Button>
            </Space.Compact>
            <div style={{ marginTop: 8 }}>
              {s.allowlist.length === 0 ? (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {t.noFolders}
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
              <Typography.Text>{t.networkOnByDefault}</Typography.Text>
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">{t.cpu}</Typography.Text>
              <InputNumber
                min={0.1}
                max={32}
                step={0.5}
                value={s.defaultCpus}
                onChange={(v) => v != null && setS({ ...s, defaultCpus: v })}
              />
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">{`${t.ram} (MB)`}</Typography.Text>
              <InputNumber
                min={64}
                max={65536}
                step={256}
                value={s.defaultMemoryMb}
                onChange={(v) => v != null && setS({ ...s, defaultMemoryMb: v })}
              />
            </Space>
            <Space size={8}>
              <Typography.Text type="secondary">{t.deadlineSeconds}</Typography.Text>
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
