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
  Radio,
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
import { useI18n, useT, type Lang } from './i18n'
import { FilesPanel } from './files'
import { MonitorPanel } from './monitor'
import { MountsPanel } from './mounts'
import { AppSettingsModal, SandboxSettings } from './settings'
import { TracePanel } from './trace'
import { SandboxTerminal } from './terminal'
import type { Resolved } from './theme'

const { Header, Content } = Layout

type Words = ReturnType<typeof useT>

/** The tab label carries the on/off state, so it is visible without opening it. */
function traceTabLabel(sb: Sandbox, t: Words): React.ReactNode {
  return sb.traceEnabled ? t.tabTraceOn : t.tabTrace
}

/**
 * Language switch. Two languages, so a segmented control beats a dropdown —
 * the alternative is always visible and one click away.
 */
function LangSwitch() {
  const { lang, setLang } = useI18n()
  // Radio buttons rather than `Segmented`: the segmented control animates a
  // sliding thumb and only re-applies its `-selected` class once that
  // transition completes. Observed here, the class never came back, so the
  // radio was checked but nothing on screen said which language was active —
  // and a language switch that does not show its own state is worse than none.
  return (
    <Radio.Group
      size="small"
      optionType="button"
      buttonStyle="solid"
      value={lang}
      onChange={(e) => setLang(e.target.value as Lang)}
      options={[
        { label: 'EN', value: 'en' },
        { label: 'VI', value: 'vi' },
      ]}
    />
  )
}

/**
 * Banner describing what this machine can actually do.
 *
 * It leads the UI rather than hiding in settings because the single most common
 * failure here is environmental — Docker installed but not running — and the
 * fix is one click in another app. Telling the user up front beats letting them
 * discover it when a run fails.
 */
function directDescription(kind: Caps['direct']['kind'], t: Words): string {
  switch (kind) {
    case 'seatbelt':
      return t.directSeatbelt
    case 'bubblewrap':
      return t.directBubblewrap
    case 'appcontainer':
      return t.directAppContainer
    case 'degraded':
      return t.directDegraded
    default:
      return t.directUnsupported
  }
}

function CapsBanner({ caps, onRefresh }: { caps: Caps; onRefresh: () => void }) {
  const t = useT()
  const refresh = (
    <Button size="small" icon={<ReloadOutlined />} onClick={onRefresh}>
      {t.checkAgain}
    </Button>
  )

  if (caps.backends.length === 0) {
    return (
      <Alert
        type="error"
        showIcon
        message={t.noBackend}
        description={
          <>
            <div>{t.directLabel} {directDescription(caps.direct.kind, t)}</div>
            <div>{t.dockerLabel} {caps.docker.detail}</div>
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
          <span>{t.backendsAvailable}</span>
          {caps.backends.map((b) => (
            <Tag key={b} color={b === 'docker' ? 'blue' : 'green'}>
              {b === 'docker' ? t.backendDocker : t.backendDirect}
            </Tag>
          ))}
        </Space>
      }
      description={
        <>
          <div>{t.directLabel} {directDescription(caps.direct.kind, t)}</div>
          <div>{t.dockerLabel} {caps.docker.detail}</div>
        </>
      }
      action={refresh}
    />
  )
}

/** One run's result. */
function RunResult({ run }: { run: Run }) {
  const t = useT()
  const iso = isolationLabel(run.isolation, t)
  return (
    <Space direction="vertical" style={{ width: '100%' }} size={8}>
      <Space wrap size={6}>
        {run.timedOut ? (
          <Tag color="orange">{t.timedOut}</Tag>
        ) : run.exitCode === 0 ? (
          <Tag color="green">{t.succeeded}</Tag>
        ) : (
          <Tag color="red">{t.exitCode(String(run.exitCode ?? '—'))}</Tag>
        )}
        <Tag color={iso.color}>{iso.text}</Tag>
        <Tag>{run.network ? t.networkOn : t.networkOff}</Tag>
        <Typography.Text type="secondary">{run.durationMs} ms</Typography.Text>
        {run.truncated && <Tag color="gold">{t.outputTruncated}</Tag>}
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
        <Typography.Text type="secondary">{t.noOutput}</Typography.Text>
      )}
    </Space>
  )
}

/** Editor + shell box for the selected sandbox. */
function RunPanel({ sandbox, languages }: { sandbox: Sandbox; languages: string[] }) {
  const { message } = AntApp.useApp()
  const t = useT()
  const [tab, setTab] = useState<'code' | 'shell'>('code')
  const [language, setLanguage] = useState('python')
  // Seeded from the dictionary so the starter snippet is in the reader's
  // language too; a later language switch leaves edited code alone.
  const [code, setCode] = useState(t.sampleCode)
  const [command, setCommand] = useState(t.sampleShell)
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
            { label: t.snippet, value: 'code' },
            { label: t.shellCommand, value: 'shell' },
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
          {t.run}
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
  const t = useT()
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
      message.success(t.created(sb.name))
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
          {t.appTitle}
        </Typography.Title>
        <Typography.Text type="secondary">{t.appTagline}</Typography.Text>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 8, alignItems: 'center' }}>
          <LangSwitch />
        </div>
        <Button
          size="small"
          icon={<SettingOutlined />}
          onClick={() => setAppSettings(true)}
        >
          {t.appDefaults}
        </Button>
      </Header>

      <Content style={{ padding: '0 20px 24px' }}>
        <Space direction="vertical" style={{ width: '100%' }} size={16}>
          {caps && <CapsBanner caps={caps} onRefresh={() => void refreshCaps(true)} />}

          <Row gutter={16}>
            <Col xs={24} md={7}>
              <Card
                size="small"
                title={t.sandboxes}
                extra={
                  <Button
                    size="small"
                    type="primary"
                    icon={<PlusOutlined />}
                    disabled={!caps || caps.backends.length === 0}
                    onClick={() => setCreating(true)}
                  >
                    {t.create}
                  </Button>
                }
              >
                {sandboxes.length === 0 ? (
                  <Empty
                    description={t.noSandboxes}
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
                            title={t.deleteSandbox(sb.name)}
                            description={t.deleteKeepsFiles}
                            okText={t.delete}
                            cancelText={t.cancel}
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
                              {sb.network && <Tag color="orange">{t.networkTag}</Tag>}
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
                  <Empty description={t.pickSandbox} />
                </Card>
              ) : (
                <Card size="small" title={current.name}>
                  <Tabs
                    items={[
                      {
                        key: 'run',
                        label: t.tabRun,
                        children: <RunPanel sandbox={current} languages={languages} />,
                      },
                      {
                        key: 'files',
                        label: t.tabFiles,
                        children: <FilesPanel sandboxId={current.id} />,
                      },
                      {
                        key: 'mon',
                        label: t.tabResources,
                        // Mounted lazily so the 2-second poll only runs while
                        // the user is actually looking at it.
                        children: <MonitorPanel sandbox={current} mode={mode} />,
                      },
                      {
                        key: 'mounts',
                        label: t.tabMounts(current.mounts.length),
                        children: (
                          <MountsPanel sandbox={current} onChange={() => void refreshList()} />
                        ),
                      },
                      {
                        key: 'term',
                        label: t.tabTerminal,
                        // Mounted lazily: opening a PTY the user never looks at
                        // would leave a shell running per sandbox.
                        children: <SandboxTerminal sandboxId={current.id} mode={mode} />,
                      },
                      {
                        key: 'trace',
                        label: traceTabLabel(current, t),
                        children: (
                          <TracePanel
                            sandbox={current}
                            onChange={() => void refreshList()}
                          />
                        ),
                      },
                      {
                        key: 'settings',
                        label: t.tabSettings,
                        children: (
                          <SandboxSettings
                            sandbox={current}
                            onChange={() => void refreshList()}
                          />
                        ),
                      },
                      {
                        key: 'hist',
                        label: t.tabHistory(history.length),
                        children:
                          history.length === 0 ? (
                            <Empty
                              description={t.traceOnEmpty}
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
                                        <Tag color={isolationLabel(r.isolation, t).color}>
                                          {isolationLabel(r.isolation, t).text}
                                        </Tag>
                                        {r.timedOut ? (
                                          <Tag color="orange">{t.timedOut}</Tag>
                                        ) : (
                                          <Tag color={r.exitCode === 0 ? 'green' : 'red'}>
                                            exit {r.exitCode ?? '—'}
                                          </Tag>
                                        )}
                                        <Typography.Text type="secondary">
                                          {r.durationMs} ms ·{' '}
                                          {new Date(r.createdAt).toLocaleString()}
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
        title={t.createTitle}
        okText={t.create}
        cancelText={t.cancel}
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
          <Form.Item name="name" label={t.name}>
            <Input placeholder={t.namePlaceholder} />
          </Form.Item>
          <Form.Item name="backend" label={t.backend}>
            <Select
              options={(caps?.backends ?? []).map((b) => ({
                value: b,
                label: b === 'docker' ? t.backendDocker : t.backendDirectLong,
              }))}
            />
          </Form.Item>
          <Form.Item
            noStyle
            shouldUpdate={(a, b) => a.backend !== b.backend}
          >
            {({ getFieldValue }) =>
              getFieldValue('backend') === 'docker' ? (
                <Form.Item name="image" label={t.dockerImage}>
                  <Input placeholder="python:3.12-slim" />
                </Form.Item>
              ) : null
            }
          </Form.Item>
          <Form.Item
            name="network"
            label={t.allowNetwork}
            valuePropName="checked"
            extra={t.allowNetworkHint}
          >
            <Switch />
          </Form.Item>
          <Space>
            <Form.Item name="cpus" label={t.cpu}>
              <InputNumber min={0.1} max={32} step={0.5} />
            </Form.Item>
            <Form.Item name="memoryMb" label={`${t.ram} (MB)`}>
              <InputNumber min={64} max={65536} step={256} />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <AppSettingsModal open={appSettings} onClose={() => setAppSettings(false)} />
    </Layout>
  )
}
