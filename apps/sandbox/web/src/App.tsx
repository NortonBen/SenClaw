import React, { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  InputNumber,
  Layout,
  List,
  Modal,
  Popconfirm,
  Row,
  Segmented,
  Select,
  Space,
  Spin,
  Switch,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import {
  CodeOutlined,
  DeleteOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  SettingOutlined,
} from '@ant-design/icons'
import { api, isolationLabel, type Caps, type Run, type Sandbox } from './api'
import { FilesPanel } from './files'
import { MonitorPanel } from './monitor'
import { MountsPanel } from './mounts'
import { AppSettingsModal, SandboxSettings } from './settings'
import { TracePanel } from './trace'
import { SandboxTerminal } from './terminal'
import type { Resolved } from './theme'

const { Header, Content } = Layout

/** The tab label carries the on/off state, so it is visible without opening it. */
function sandbox_trace_label(sb: Sandbox): React.ReactNode {
  return sb.traceEnabled ? 'Theo dõi ●' : 'Theo dõi'
}

/**
 * Banner describing what this machine can actually do.
 *
 * It leads the UI rather than hiding in settings because the single most common
 * failure here is environmental — Docker installed but not running — and the
 * fix is one click in another app. Telling the user up front beats letting them
 * discover it when a run fails.
 */
function CapsBanner({ caps, onRefresh }: { caps: Caps; onRefresh: () => void }) {
  const refresh = (
    <Button size="small" icon={<ReloadOutlined />} onClick={onRefresh}>
      Kiểm tra lại
    </Button>
  )

  if (caps.backends.length === 0) {
    return (
      <Alert
        type="error"
        showIcon
        message="Máy này chưa chạy được sandbox nào"
        description={
          <>
            <div>Trực tiếp: {caps.direct.detail}</div>
            <div>Docker: {caps.docker.detail}</div>
          </>
        }
        action={refresh}
      />
    )
  }

  const degraded = caps.direct.available && caps.direct.kind === 'degraded'
  return (
    <Alert
      type={degraded ? 'warning' : 'success'}
      showIcon
      message={
        <Space wrap size={6}>
          <span>Backend dùng được:</span>
          {caps.backends.map((b) => (
            <Tag key={b} color={b === 'docker' ? 'blue' : 'green'}>
              {b === 'docker' ? 'Docker container' : 'Chạy trực tiếp'}
            </Tag>
          ))}
        </Space>
      }
      description={
        <>
          <div>Trực tiếp: {caps.direct.detail}</div>
          <div>Docker: {caps.docker.detail}</div>
        </>
      }
      action={refresh}
    />
  )
}

/** One run's result. */
function RunResult({ run }: { run: Run }) {
  const iso = isolationLabel(run.isolation)
  return (
    <Space direction="vertical" style={{ width: '100%' }} size={8}>
      <Space wrap size={6}>
        {run.timedOut ? (
          <Tag color="orange">Quá giờ</Tag>
        ) : run.exitCode === 0 ? (
          <Tag color="green">Thành công</Tag>
        ) : (
          <Tag color="red">Mã thoát {run.exitCode ?? '—'}</Tag>
        )}
        <Tag color={iso.color}>{iso.text}</Tag>
        <Tag>{run.network ? 'Mạng bật' : 'Mạng tắt'}</Tag>
        <Typography.Text type="secondary">{run.durationMs} ms</Typography.Text>
        {run.truncated && <Tag color="gold">Output đã bị cắt</Tag>}
      </Space>
      {run.stdout && (
        <pre className="sbx-mono sbx-output" style={{ background: 'var(--sbx-code-bg)' }}>
          {run.stdout}
        </pre>
      )}
      {run.stderr && (
        <pre
          className="sbx-mono sbx-output"
          style={{ background: 'var(--sbx-code-bg)', color: '#e05252' }}
        >
          {run.stderr}
        </pre>
      )}
      {!run.stdout && !run.stderr && (
        <Typography.Text type="secondary">(không có output)</Typography.Text>
      )}
    </Space>
  )
}

/** Editor + shell box for the selected sandbox. */
function RunPanel({ sandbox, languages }: { sandbox: Sandbox; languages: string[] }) {
  const { message } = AntApp.useApp()
  const [tab, setTab] = useState<'code' | 'shell'>('code')
  const [language, setLanguage] = useState('python')
  const [code, setCode] = useState('print("xin chào từ sandbox")\n')
  const [command, setCommand] = useState('ls -la\n')
  const [busy, setBusy] = useState(false)
  const [run, setRun] = useState<Run | null>(null)

  const go = async () => {
    setBusy(true)
    try {
      const r =
        tab === 'code'
          ? await api.runCode(sandbox.id, language, code)
          : await api.exec(sandbox.id, command)
      setRun(r)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={12}>
      <Space wrap>
        <Segmented
          value={tab}
          onChange={(v) => setTab(v as 'code' | 'shell')}
          options={[
            { label: 'Đoạn mã', value: 'code' },
            { label: 'Lệnh shell', value: 'shell' },
          ]}
        />
        {tab === 'code' && (
          <Select
            value={language}
            onChange={setLanguage}
            style={{ width: 150 }}
            options={languages.map((l) => ({ value: l, label: l }))}
          />
        )}
        <Button
          type="primary"
          icon={<PlayCircleOutlined />}
          loading={busy}
          onClick={() => void go()}
        >
          Chạy
        </Button>
      </Space>

      <Input.TextArea
        className="sbx-mono sbx-editor"
        rows={10}
        value={tab === 'code' ? code : command}
        onChange={(e) => (tab === 'code' ? setCode(e.target.value) : setCommand(e.target.value))}
        spellCheck={false}
      />

      {busy && <Spin />}
      {run && <RunResult run={run} />}
    </Space>
  )
}

export default function App({ mode }: { mode: Resolved }) {
  const { message } = AntApp.useApp()
  const [caps, setCaps] = useState<Caps | null>(null)
  const [sandboxes, setSandboxes] = useState<Sandbox[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [languages, setLanguages] = useState<string[]>(['python'])
  const [creating, setCreating] = useState(false)
  const [appSettings, setAppSettings] = useState(false)
  const [history, setHistory] = useState<Run[]>([])
  const [form] = Form.useForm()

  const refreshCaps = useCallback(
    async (force = false) => {
      try {
        setCaps(await api.caps(force))
      } catch (e) {
        message.error((e as Error).message)
      }
    },
    [message],
  )

  const refreshList = useCallback(async () => {
    try {
      const r = await api.listSandboxes()
      setSandboxes(r.sandboxes)
      setSelected((cur) => cur ?? r.sandboxes[0]?.id ?? null)
    } catch (e) {
      message.error((e as Error).message)
    }
  }, [message])

  useEffect(() => {
    void refreshCaps()
    void refreshList()
    api.languages().then((r) => setLanguages(r.languages)).catch(() => {})
  }, [refreshCaps, refreshList])

  const current = useMemo(
    () => sandboxes.find((s) => s.id === selected) ?? null,
    [sandboxes, selected],
  )

  useEffect(() => {
    if (!selected) return void setHistory([])
    api.runs(selected, 30).then((r) => setHistory(r.runs)).catch(() => {})
  }, [selected])

  const create = async () => {
    const v = await form.validateFields()
    try {
      const sb = await api.createSandbox(v)
      setCreating(false)
      form.resetFields()
      await refreshList()
      setSelected(sb.id)
      message.success(`Đã tạo sandbox "${sb.name}"`)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const remove = async (sb: Sandbox, purge: boolean) => {
    try {
      await api.deleteSandbox(sb.id, purge)
      setSelected(null)
      await refreshList()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <Layout style={{ minHeight: '100vh', background: 'transparent' }}>
      <Header
        style={{
          background: 'transparent',
          padding: '0 20px',
          display: 'flex',
          alignItems: 'center',
          gap: 10,
        }}
      >
        <CodeOutlined style={{ fontSize: 20, color: '#00a37a' }} />
        <Typography.Title level={4} style={{ margin: 0 }}>
          Sandbox
        </Typography.Title>
        <Typography.Text type="secondary">chạy lệnh cách ly khỏi máy thật</Typography.Text>
        <Button
          size="small"
          icon={<SettingOutlined />}
          style={{ marginLeft: 'auto' }}
          onClick={() => setAppSettings(true)}
        >
          Cài đặt mặc định
        </Button>
      </Header>

      <Content style={{ padding: '0 20px 24px' }}>
        <Space direction="vertical" style={{ width: '100%' }} size={16}>
          {caps && <CapsBanner caps={caps} onRefresh={() => void refreshCaps(true)} />}

          <Row gutter={16}>
            <Col xs={24} md={7}>
              <Card
                size="small"
                title="Sandbox"
                extra={
                  <Button
                    size="small"
                    type="primary"
                    icon={<PlusOutlined />}
                    disabled={!caps || caps.backends.length === 0}
                    onClick={() => setCreating(true)}
                  >
                    Tạo
                  </Button>
                }
              >
                {sandboxes.length === 0 ? (
                  <Empty
                    description="Chưa có sandbox nào"
                    image={Empty.PRESENTED_IMAGE_SIMPLE}
                  />
                ) : (
                  <List
                    size="small"
                    dataSource={sandboxes}
                    renderItem={(sb) => (
                      <List.Item
                        style={{
                          cursor: 'pointer',
                          background:
                            sb.id === selected ? 'var(--sbx-code-bg)' : undefined,
                          borderRadius: 6,
                          paddingInline: 8,
                        }}
                        onClick={() => setSelected(sb.id)}
                        actions={[
                          <Popconfirm
                            key="d"
                            title={`Xoá "${sb.name}"?`}
                            description="File trong sandbox vẫn được giữ lại."
                            okText="Xoá"
                            cancelText="Thôi"
                            onConfirm={() => void remove(sb, false)}
                          >
                            <Button
                              size="small"
                              type="text"
                              danger
                              icon={<DeleteOutlined />}
                              onClick={(e) => e.stopPropagation()}
                            />
                          </Popconfirm>,
                        ]}
                      >
                        <List.Item.Meta
                          title={sb.name}
                          description={
                            <Space size={4} wrap>
                              <Tag color={sb.backend === 'docker' ? 'blue' : 'green'}>
                                {sb.backend}
                              </Tag>
                              {sb.network && <Tag color="orange">mạng</Tag>}
                              <Typography.Text type="secondary">
                                {sb.memoryMb} MB
                              </Typography.Text>
                            </Space>
                          }
                        />
                      </List.Item>
                    )}
                  />
                )}
              </Card>
            </Col>

            <Col xs={24} md={17}>
              {!current ? (
                <Card size="small">
                  <Empty description="Chọn hoặc tạo một sandbox để bắt đầu" />
                </Card>
              ) : (
                <Card size="small" title={current.name}>
                  <Tabs
                    items={[
                      {
                        key: 'run',
                        label: 'Chạy',
                        children: <RunPanel sandbox={current} languages={languages} />,
                      },
                      {
                        key: 'files',
                        label: 'File',
                        children: <FilesPanel sandboxId={current.id} />,
                      },
                      {
                        key: 'mon',
                        label: 'Tài nguyên',
                        // Mounted lazily so the 2-second poll only runs while
                        // the user is actually looking at it.
                        children: <MonitorPanel sandbox={current} mode={mode} />,
                      },
                      {
                        key: 'mounts',
                        label: `Thư mục gắn (${current.mounts.length})`,
                        children: (
                          <MountsPanel sandbox={current} onChange={() => void refreshList()} />
                        ),
                      },
                      {
                        key: 'term',
                        label: 'Terminal',
                        // Mounted lazily: opening a PTY the user never looks at
                        // would leave a shell running per sandbox.
                        children: <SandboxTerminal sandboxId={current.id} mode={mode} />,
                      },
                      {
                        key: 'trace',
                        label: sandbox_trace_label(current),
                        children: (
                          <TracePanel
                            sandbox={current}
                            onChange={() => void refreshList()}
                          />
                        ),
                      },
                      {
                        key: 'settings',
                        label: 'Cài đặt',
                        children: (
                          <SandboxSettings
                            sandbox={current}
                            onChange={() => void refreshList()}
                          />
                        ),
                      },
                      {
                        key: 'hist',
                        label: `Lịch sử (${history.length})`,
                        children:
                          history.length === 0 ? (
                            <Empty
                              description="Chưa chạy lần nào"
                              image={Empty.PRESENTED_IMAGE_SIMPLE}
                            />
                          ) : (
                            <List
                              size="small"
                              dataSource={history}
                              renderItem={(r) => (
                                <List.Item>
                                  <List.Item.Meta
                                    title={
                                      <Typography.Text className="sbx-mono" ellipsis>
                                        {r.source.split('\n')[0]}
                                      </Typography.Text>
                                    }
                                    description={
                                      <Space size={4} wrap>
                                        <Tag color={isolationLabel(r.isolation).color}>
                                          {isolationLabel(r.isolation).text}
                                        </Tag>
                                        {r.timedOut ? (
                                          <Tag color="orange">quá giờ</Tag>
                                        ) : (
                                          <Tag color={r.exitCode === 0 ? 'green' : 'red'}>
                                            exit {r.exitCode ?? '—'}
                                          </Tag>
                                        )}
                                        <Typography.Text type="secondary">
                                          {r.durationMs} ms ·{' '}
                                          {new Date(r.createdAt).toLocaleString('vi-VN')}
                                        </Typography.Text>
                                      </Space>
                                    }
                                  />
                                </List.Item>
                              )}
                            />
                          ),
                      },
                    ]}
                  />
                </Card>
              )}
            </Col>
          </Row>
        </Space>
      </Content>

      <Modal
        open={creating}
        title="Tạo sandbox"
        okText="Tạo"
        cancelText="Thôi"
        onOk={() => void create()}
        onCancel={() => setCreating(false)}
      >
        <Form
          form={form}
          layout="vertical"
          initialValues={{
            backend: caps?.backends[0],
            network: false,
            cpus: 1,
            memoryMb: 512,
          }}
        >
          <Form.Item name="name" label="Tên">
            <Input placeholder="để trống thì tự đặt" />
          </Form.Item>
          <Form.Item name="backend" label="Backend">
            <Select
              options={(caps?.backends ?? []).map((b) => ({
                value: b,
                label: b === 'docker' ? 'Docker container' : 'Chạy trực tiếp (OS sandbox)',
              }))}
            />
          </Form.Item>
          <Form.Item
            noStyle
            shouldUpdate={(a, b) => a.backend !== b.backend}
          >
            {({ getFieldValue }) =>
              getFieldValue('backend') === 'docker' ? (
                <Form.Item name="image" label="Docker image">
                  <Input placeholder="python:3.12-slim" />
                </Form.Item>
              ) : null
            }
          </Form.Item>
          <Form.Item
            name="network"
            label="Cho phép mạng"
            valuePropName="checked"
            extra="Tắt là an toàn hơn. Phải bật thì mới cài được gói."
          >
            <Switch />
          </Form.Item>
          <Space>
            <Form.Item name="cpus" label="CPU">
              <InputNumber min={0.1} max={32} step={0.5} />
            </Form.Item>
            <Form.Item name="memoryMb" label="RAM (MB)">
              <InputNumber min={64} max={65536} step={256} />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <AppSettingsModal open={appSettings} onClose={() => setAppSettings(false)} />
    </Layout>
  )
}
