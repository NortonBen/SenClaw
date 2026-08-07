import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Badge,
  Button,
  Descriptions,
  Dropdown,
  Empty,
  Flex,
  Input,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Spin,
  Switch,
  Tabs,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import type { MenuProps } from 'antd'
import { api, fmtTime, type CliInfo, type Workspace, type WsDetail } from './api'
import { AddWorkspaceModal } from './addws'
import { ConsoleDrawer, RunsTab, StatusTag } from './console'
import { TfvarsPickerModal } from './tfpicker'
import { VarsForm } from './vars'

const { Title, Text } = Typography

function WsStatusDot({ ws }: { ws: Workspace }) {
  if (ws.status === 'cloning') return <Badge status="processing" />
  if (ws.status === 'error') return <Badge status="error" />
  return <Badge status="success" />
}

function InfoPane({
  detail,
  onChanged,
  onRun,
  onDeleted,
}: {
  detail: WsDetail
  onChanged: () => void
  onRun: (id: number) => void
  onDeleted: () => void
}) {
  const { message } = AntApp.useApp()
  const ws = detail.workspace
  const [name, setName] = useState(ws.name)
  const [subdir, setSubdir] = useState(ws.subdir)
  const [subdirOpts, setSubdirOpts] = useState<string[]>([])
  const [varFile, setVarFile] = useState(ws.var_file)
  const [pickerOpen, setPickerOpen] = useState(false)
  useEffect(() => setName(ws.name), [ws.id, ws.name])
  useEffect(() => setVarFile(ws.var_file), [ws.id, ws.var_file])

  const saveVarFile = async (f: string) => {
    try {
      await api.wsPatch(ws.id, { var_file: f })
      message.success(f ? `Plan/apply sẽ dùng ${f}` : 'Đã bỏ var-file mặc định')
      onChanged()
    } catch (e) {
      message.error(String(e))
    }
  }
  useEffect(() => {
    setSubdir(ws.subdir)
    api.subdirs(ws.id).then((r) => setSubdirOpts(r.subdirs)).catch(() => setSubdirOpts([]))
  }, [ws.id, ws.subdir])

  const sync = async () => {
    try {
      const r = await api.wsSync(ws.id)
      onRun(r.run_id)
    } catch (e) {
      message.error(String(e))
    }
  }

  return (
    <div style={{ maxWidth: 720 }}>
      <Descriptions
        size="small"
        column={1}
        bordered
        items={[
          {
            key: 'src',
            label: 'Nguồn',
            children:
              ws.source === 'git' ? (
                <Space>
                  <Tag color="purple">git</Tag>
                  <Text code>{ws.repo_url}</Text>
                  {ws.branch && <Tag>{ws.branch}</Tag>}
                </Space>
              ) : (
                <Tag color="blue">thư mục local</Tag>
              ),
          },
          {
            key: 'dir',
            label: 'Thư mục',
            children: (
              <Space>
                <Text code>{ws.dir}</Text>
                <Button
                  size="small"
                  onClick={() => api.openDir(ws.id).catch((e) => message.error(String(e)))}
                >
                  📂 Mở
                </Button>
              </Space>
            ),
          },
          {
            key: 'subdir',
            label: 'Thư mục Terraform',
            children: (
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                <Space.Compact style={{ minWidth: 340 }}>
                  <Select
                    style={{ minWidth: 260 }}
                    value={subdir}
                    onChange={setSubdir}
                    options={[
                      { value: '', label: '(gốc repo)' },
                      ...Array.from(new Set([...subdirOpts, ...(ws.subdir ? [ws.subdir] : [])])).map(
                        (s) => ({ value: s, label: `📁 ${s}` }),
                      ),
                    ]}
                  />
                  <Button
                    onClick={async () => {
                      try {
                        await api.wsPatch(ws.id, { subdir })
                        message.success(subdir ? `Root Terraform: ${subdir}` : 'Dùng gốc repo')
                        onChanged()
                      } catch (e) {
                        message.error(String(e))
                      }
                    }}
                  >
                    Lưu
                  </Button>
                </Space.Compact>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  Terraform chạy tại: <Text code>{detail.work_dir}</Text>
                  {!detail.work_dir_exists && <Tag color="red" style={{ marginLeft: 6 }}>không tồn tại</Tag>}
                </Text>
              </Space>
            ),
          },
          {
            key: 'varfile',
            label: 'File giá trị (.tfvars)',
            children: (
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                <Space.Compact style={{ minWidth: 340 }}>
                  <Select
                    style={{ minWidth: 260 }}
                    value={varFile}
                    onChange={setVarFile}
                    options={[
                      { value: '', label: '(không dùng var-file)' },
                      ...Array.from(
                        new Set([...detail.tfvars_files, ...(ws.var_file ? [ws.var_file] : [])]),
                      ).map((f) => ({ value: f, label: f })),
                    ]}
                    notFoundContent="Chưa có file .tfvars trong thư mục Terraform — bấm 📂 Chọn…"
                  />
                  <Button onClick={() => setPickerOpen(true)}>📂 Chọn…</Button>
                  <Button onClick={() => saveVarFile(varFile)}>Lưu</Button>
                </Space.Compact>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  plan / apply / destroy tự truyền <Text code>-var-file={varFile || '…'}</Text>.
                  Tạo file mới ở tab Biến &amp; Chạy (nút &quot;File mới&quot;).
                </Text>
              </Space>
            ),
          },
          ...(detail.git.is_git
            ? [
                {
                  key: 'git',
                  label: 'Git',
                  children: (
                    <Space direction="vertical" size={2}>
                      <span>
                        nhánh <Text code>{detail.git.branch ?? '?'}</Text>
                        {detail.git.dirty_files ? (
                          <Tag color="orange" style={{ marginLeft: 8 }}>
                            {detail.git.dirty_files} file sửa local
                          </Tag>
                        ) : (
                          <Tag color="green" style={{ marginLeft: 8 }}>
                            sạch
                          </Tag>
                        )}
                      </span>
                      <Text type="secondary">{detail.git.commit}</Text>
                    </Space>
                  ),
                },
              ]
            : []),
          {
            key: 'init',
            label: 'Terraform',
            children: detail.initialized ? (
              <Tag color="green">đã init (.terraform có sẵn)</Tag>
            ) : (
              <Tag>chưa init — lần chạy đầu sẽ tự init</Tag>
            ),
          },
          {
            key: 'last',
            label: 'Run gần nhất',
            children: detail.last_run ? (
              <Space>
                <Text code>{detail.last_run.kind}</Text>
                <StatusTag status={detail.last_run.status} />
                <Text type="secondary">{fmtTime(detail.last_run.started_at)}</Text>
              </Space>
            ) : (
              '—'
            ),
          },
        ]}
      />

      <Space style={{ marginTop: 16 }} wrap>
        {detail.git.is_git && (
          <>
            <Button onClick={sync}>🔄 Sync (git pull) ngay</Button>
            <Tooltip title="Tự git pull --ff-only trước mỗi plan/apply/destroy">
              <Space>
                <Switch
                  checked={ws.auto_sync}
                  onChange={async (v) => {
                    await api.wsPatch(ws.id, { auto_sync: v })
                    onChanged()
                  }}
                />
                <Text>tự sync trước khi chạy</Text>
              </Space>
            </Tooltip>
          </>
        )}
      </Space>

      <Space.Compact style={{ marginTop: 16, width: 360, display: 'flex' }}>
        <Input value={name} onChange={(e) => setName(e.target.value)} addonBefore="Tên" />
        <Button
          onClick={async () => {
            await api.wsPatch(ws.id, { name })
            message.success('Đã đổi tên')
            onChanged()
          }}
        >
          Lưu
        </Button>
      </Space.Compact>

      <div style={{ marginTop: 24 }}>
        <Popconfirm
          title="Xoá workspace này?"
          description={
            ws.source === 'git'
              ? 'Bản clone app quản lý sẽ bị xoá khỏi đĩa. Hạ tầng thật KHÔNG bị đụng tới.'
              : 'Chỉ gỡ khỏi app — thư mục của bạn giữ nguyên. Hạ tầng thật KHÔNG bị đụng tới.'
          }
          okText="Xoá"
          okButtonProps={{ danger: true }}
          cancelText="Thôi"
          onConfirm={async () => {
            try {
              await api.wsDelete(ws.id)
              message.success('Đã xoá workspace')
              onDeleted()
            } catch (e) {
              message.error(String(e))
            }
          }}
        >
          <Button danger>Xoá workspace</Button>
        </Popconfirm>
      </div>

      <TfvarsPickerModal
        wsId={ws.id}
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        onPicked={(rel) => {
          setVarFile(rel)
          saveVarFile(rel)
        }}
      />
    </div>
  )
}

export default function App({
  themePref,
  onThemePref,
}: {
  themePref: 'auto' | 'dark' | 'light'
  onThemePref: (v: 'auto' | 'dark' | 'light') => void
}) {
  const { message, modal } = AntApp.useApp()
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])
  const [selId, setSelId] = useState<number | null>(null)
  const [detail, setDetail] = useState<WsDetail | null>(null)
  const [cli, setCli] = useState<CliInfo | null>(null)
  const [consoleRun, setConsoleRun] = useState<number | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [tab, setTab] = useState('vars')

  const loadWorkspaces = useCallback(async () => {
    try {
      const r = await api.workspaces()
      setWorkspaces(r.workspaces)
    } catch {
      /* backend chưa dậy — thử lại vòng sau */
    }
  }, [])

  const loadCli = useCallback(() => api.cli().then(setCli).catch(() => {}), [])

  const loadDetail = useCallback(async () => {
    if (selId == null) {
      setDetail(null)
      return
    }
    try {
      setDetail(await api.wsGet(selId))
    } catch {
      setDetail(null)
    }
  }, [selId])

  useEffect(() => {
    loadWorkspaces()
    loadCli()
    const t = setInterval(loadWorkspaces, 5000)
    return () => clearInterval(t)
  }, [loadWorkspaces, loadCli])

  useEffect(() => {
    loadDetail()
    const t = setInterval(loadDetail, 5000)
    return () => clearInterval(t)
  }, [loadDetail])

  const installCli = () => {
    modal.confirm({
      title: 'Cài Terraform CLI?',
      content:
        'App sẽ tải bản chính thức mới nhất từ releases.hashicorp.com về thư mục app (~/.senclaw/apps/terraform/bin) — không đụng hệ thống. Hỗ trợ macOS/Linux/Windows.',
      okText: 'Cài ngay',
      cancelText: 'Thôi',
      onOk: async () => {
        try {
          const r = await api.cliInstall()
          setConsoleRun(r.run_id)
        } catch (e) {
          message.error(String(e))
        }
      },
    })
  }

  const selected = workspaces.find((w) => w.id === selId) ?? null

  const confirmDelete = (w: Workspace) => {
    modal.confirm({
      title: `Xoá workspace "${w.name}"?`,
      content:
        w.source === 'git'
          ? 'Bản clone app quản lý sẽ bị xoá khỏi đĩa. Hạ tầng thật KHÔNG bị đụng tới.'
          : 'Chỉ gỡ khỏi app — thư mục của bạn giữ nguyên. Hạ tầng thật KHÔNG bị đụng tới.',
      okText: 'Xoá',
      okButtonProps: { danger: true },
      cancelText: 'Thôi',
      onOk: async () => {
        try {
          await api.wsDelete(w.id)
          message.success('Đã xoá workspace')
          if (selId === w.id) setSelId(null)
          loadWorkspaces()
        } catch (e) {
          message.error(String(e))
        }
      },
    })
  }

  // Menu chuột phải trên card workspace.
  const wsMenu = (w: Workspace): MenuProps => ({
    items: [
      {
        key: 'open',
        label: w.source === 'git' ? '📂 Mở thư mục đã clone' : '📂 Mở thư mục',
      },
      { key: 'copy', label: '📋 Copy đường dẫn' },
      ...(w.source === 'git' ? [{ key: 'sync', label: '🔄 Sync (git pull)' }] : []),
      { type: 'divider' as const },
      { key: 'del', label: '🗑 Xoá workspace', danger: true },
    ],
    onClick: ({ key, domEvent }) => {
      domEvent.stopPropagation()
      if (key === 'open') {
        api.openDir(w.id)
          .then((r) => message.success(`Đã mở ${r.dir}`))
          .catch((e) => message.error(String(e)))
      } else if (key === 'copy') {
        navigator.clipboard
          ?.writeText(w.dir)
          .then(() => message.success('Đã copy đường dẫn'))
          .catch(() => {
            modal.info({ title: 'Đường dẫn', content: <Text code copyable>{w.dir}</Text> })
          })
      } else if (key === 'sync') {
        api.wsSync(w.id)
          .then((r) => setConsoleRun(r.run_id))
          .catch((e) => message.error(String(e)))
      } else if (key === 'del') {
        confirmDelete(w)
      }
    },
  })

  return (
    <div style={{ padding: 20, minHeight: '100vh' }}>
      <Flex align="center" justify="space-between" style={{ marginBottom: 12 }}>
        <Title level={3} style={{ margin: 0 }}>
          🏗️ Terraform{' '}
          <Text type="secondary" style={{ fontSize: 14 }}>
            — SenClaw
          </Text>
        </Title>
        <Space>
          <Segmented
            size="small"
            value={themePref}
            onChange={(v) => onThemePref(v as 'auto' | 'dark' | 'light')}
            options={[
              { value: 'auto', label: 'Auto' },
              { value: 'light', label: '☀️' },
              { value: 'dark', label: '🌙' },
            ]}
          />
          {cli &&
            (cli.found ? (
              <Tooltip title={`${cli.path} (${cli.source})`}>
                <Tag color="green">terraform v{cli.version ?? '?'}</Tag>
              </Tooltip>
            ) : (
              <Space>
                <Tag color="red">chưa có Terraform CLI</Tag>
                <Button size="small" type="primary" onClick={installCli}>
                  ⬇ Cài Terraform
                </Button>
              </Space>
            ))}
        </Space>
      </Flex>

      {cli && !cli.found && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="Máy chưa có Terraform CLI — vẫn xem/sửa biến được, nhưng cần cài để chạy plan/apply."
          action={
            <Button size="small" type="primary" onClick={installCli}>
              Cài tự động
            </Button>
          }
        />
      )}

      <Flex gap={16} align="flex-start">
        <div style={{ width: 260, flexShrink: 0 }}>
          <Button type="primary" block onClick={() => setAddOpen(true)} style={{ marginBottom: 10 }}>
            ➕ Thêm workspace
          </Button>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {workspaces.map((w) => (
              <Dropdown key={w.id} menu={wsMenu(w)} trigger={['contextMenu']}>
              <div
                onClick={() => {
                  setSelId(w.id)
                  setTab('vars')
                }}
                className={'ws-card' + (w.id === selId ? ' sel' : '')}
              >
                <Space size={8}>
                  <WsStatusDot ws={w} />
                  <span>{w.source === 'git' ? '🔗' : '📁'}</span>
                  <Text strong>{w.name}</Text>
                </Space>
                <div>
                  <Text type="secondary" style={{ fontSize: 11 }} ellipsis>
                    {w.source === 'git' ? w.repo_url : w.dir}
                  </Text>
                </div>
                {w.subdir && (
                  <Tag style={{ marginTop: 4, fontSize: 11 }} color="purple">
                    📁 {w.subdir}
                  </Tag>
                )}
                {w.var_file && (
                  <Tag style={{ marginTop: 4, fontSize: 11 }} color="geekblue">
                    {w.var_file}
                  </Tag>
                )}
              </div>
              </Dropdown>
            ))}
            {workspaces.length === 0 && (
              <Text type="secondary" style={{ padding: 8 }}>
                Chưa có workspace nào.
              </Text>
            )}
          </div>
        </div>

        <div style={{ flex: 1, minWidth: 0 }}>
          {selected == null ? (
            <Empty
              style={{ marginTop: 60 }}
              description="Chọn hoặc thêm một workspace Terraform — từ thư mục trên máy, hoặc clone từ git"
            >
              <Button type="primary" onClick={() => setAddOpen(true)}>
                ➕ Thêm workspace
              </Button>
            </Empty>
          ) : detail == null ? (
            <Spin style={{ display: 'block', margin: '60px auto' }} />
          ) : (
            <>
              {selected.status === 'error' && (
                <Alert
                  type="error"
                  showIcon
                  style={{ marginBottom: 12 }}
                  message="Workspace lỗi"
                  description={selected.last_error}
                />
              )}
              {detail.running_run != null && (
                <Alert
                  type="info"
                  showIcon
                  style={{ marginBottom: 12 }}
                  message={
                    <Space>
                      <span>Đang có run chạy…</span>
                      <Button size="small" onClick={() => setConsoleRun(detail.running_run)}>
                        Xem console
                      </Button>
                    </Space>
                  }
                />
              )}
              <Tabs
                activeKey={tab}
                onChange={setTab}
                items={[
                  {
                    key: 'vars',
                    label: '📝 Biến & Chạy',
                    children: (
                      <VarsForm
                        ws={selected}
                        onRun={(id) => {
                          setConsoleRun(id)
                          loadDetail()
                        }}
                        onChanged={() => {
                          loadWorkspaces()
                          loadDetail()
                        }}
                      />
                    ),
                  },
                  {
                    key: 'runs',
                    label: '🖥 Console',
                    children: <RunsTab workspaceId={selected.id} onOpenRun={setConsoleRun} />,
                  },
                  {
                    key: 'info',
                    label: 'ℹ️ Thông tin',
                    children: (
                      <InfoPane
                        detail={detail}
                        onChanged={() => {
                          loadWorkspaces()
                          loadDetail()
                        }}
                        onRun={setConsoleRun}
                        onDeleted={() => {
                          setSelId(null)
                          loadWorkspaces()
                        }}
                      />
                    ),
                  },
                ]}
              />
            </>
          )}
        </div>
      </Flex>

      <AddWorkspaceModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onAdded={(ws, runId) => {
          setAddOpen(false)
          loadWorkspaces()
          setSelId(ws.id)
          if (runId != null) setConsoleRun(runId)
        }}
      />
      <ConsoleDrawer
        runId={consoleRun}
        onClose={() => {
          setConsoleRun(null)
          loadDetail()
          loadCli()
        }}
      />
    </div>
  )
}
