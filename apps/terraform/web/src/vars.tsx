// Form Apply render từ variables.tf + chọn/điền/lưu file .tfvars, và hàng nút
// chạy terraform (plan/apply/destroy đi kèm var-file đã chọn).
import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Checkbox,
  Divider,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Spin,
  Switch,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import { api, type VarDef, type Workspace } from './api'
import { TfvarsPickerModal } from './tfpicker'

const { Text } = Typography

type Kind = 'bool' | 'number' | 'json' | 'secret' | 'text'

function controlKind(def: VarDef): Kind {
  const t = def.var_type.trim()
  if (def.sensitive) return 'secret'
  if (t === 'bool') return 'bool'
  if (t === 'number') return 'number'
  if (/^(list|map|set|object|tuple|any)/.test(t)) return 'json'
  if (def.default != null && typeof def.default === 'object') return 'json'
  return 'text'
}

function initValue(def: VarDef, fileVal: unknown): unknown {
  const kind = controlKind(def)
  switch (kind) {
    case 'bool':
      return typeof fileVal === 'boolean' ? fileVal : (def.default as boolean | null) ?? false
    case 'number':
      return typeof fileVal === 'number' ? fileVal : (def.default as number | null) ?? null
    case 'json':
      if (fileVal !== undefined) return JSON.stringify(fileVal, null, 2)
      return def.default != null ? JSON.stringify(def.default, null, 2) : ''
    default:
      return fileVal != null ? String(fileVal) : ''
  }
}

export function VarsForm({
  ws,
  onRun,
  onChanged,
}: {
  ws: Workspace
  onRun: (runId: number) => void
  onChanged: () => void
}) {
  const { message, modal } = AntApp.useApp()
  const [defs, setDefs] = useState<VarDef[]>([])
  const [parseErrors, setParseErrors] = useState<string[]>([])
  const [files, setFiles] = useState<string[]>([])
  const [file, setFile] = useState<string>('')
  const [values, setValues] = useState<Record<string, unknown>>({})
  const [extraValues, setExtraValues] = useState<Record<string, unknown>>({})
  const [replace, setReplace] = useState(false)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [running, setRunning] = useState<string | null>(null)
  const [newFileOpen, setNewFileOpen] = useState(false)
  const [newFileName, setNewFileName] = useState('prod.tfvars')
  const [subdirOpts, setSubdirOpts] = useState<string[] | null>(null)
  const [subdirPick, setSubdirPick] = useState<string>('')
  const [subdirSaving, setSubdirSaving] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)

  const loadValues = useCallback(
    async (defsNow: VarDef[], f: string) => {
      let fileVals: Record<string, unknown> = {}
      if (f) {
        try {
          const r = await api.tfvarsGet(ws.id, f)
          fileVals = r.values ?? {}
        } catch (e) {
          message.error(String(e))
        }
      }
      const next: Record<string, unknown> = {}
      for (const d of defsNow) next[d.name] = initValue(d, fileVals[d.name])
      setValues(next)
      // Biến có trong tfvars nhưng không khai trong *.tf — giữ để không mất khi lưu.
      const extra: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(fileVals)) {
        if (!defsNow.some((d) => d.name === k)) extra[k] = v
      }
      setExtraValues(extra)
    },
    [ws.id, message],
  )

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const r = await api.variables(ws.id)
      setDefs(r.variables)
      setParseErrors(r.parse_errors)
      setFiles(r.tfvars_files)
      const f = r.var_file || r.tfvars_files[0] || ''
      setFile(f)
      await loadValues(r.variables, f)
      // Không thấy biến nào → có thể *.tf nằm trong thư mục con: dò gợi ý.
      if (r.variables.length === 0) {
        try {
          const sd = await api.subdirs(ws.id)
          setSubdirOpts(sd.subdirs)
          setSubdirPick(sd.subdirs[0] ?? '')
        } catch {
          setSubdirOpts([])
        }
      } else {
        setSubdirOpts(null)
      }
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }, [ws.id, loadValues, message])

  useEffect(() => {
    load()
  }, [load])

  const changeFile = async (f: string) => {
    setFile(f)
    await loadValues(defs, f)
  }

  const buildPayload = (): Record<string, unknown> | null => {
    const out: Record<string, unknown> = {}
    for (const d of defs) {
      const kind = controlKind(d)
      const v = values[d.name]
      if (kind === 'bool') out[d.name] = Boolean(v)
      else if (kind === 'number') {
        if (v != null && v !== '') out[d.name] = Number(v)
      } else if (kind === 'json') {
        const text = String(v ?? '').trim()
        if (!text) continue
        try {
          out[d.name] = JSON.parse(text)
        } catch {
          message.error(`Biến "${d.name}": JSON không hợp lệ`)
          return null
        }
      } else {
        const text = String(v ?? '')
        if (text !== '') out[d.name] = text
      }
    }
    if (!replace) {
      for (const [k, v] of Object.entries(extraValues)) if (!(k in out)) out[k] = v
    }
    return out
  }

  const save = async (): Promise<boolean> => {
    if (!file) {
      setNewFileOpen(true)
      return false
    }
    const payload = buildPayload()
    if (!payload) return false
    setSaving(true)
    try {
      await api.tfvarsSet(ws.id, file, payload)
      message.success(`Đã lưu ${file} (${Object.keys(payload).length} biến)`)
      if (!files.includes(file)) setFiles([...files, file])
      onChanged()
      return true
    } catch (e) {
      message.error(String(e))
      return false
    } finally {
      setSaving(false)
    }
  }

  const runCmd = async (command: string, confirm = false) => {
    setRunning(command)
    try {
      const r = await api.run(ws.id, command, {
        confirm,
        var_file: file || undefined,
      })
      onRun(r.run_id)
    } catch (e) {
      message.error(String(e))
    } finally {
      setRunning(null)
    }
  }

  const confirmRun = (command: 'apply' | 'destroy') => {
    const danger = command === 'destroy'
    modal.confirm({
      title: danger ? 'XOÁ toàn bộ hạ tầng?' : 'Apply thay đổi hạ tầng thật?',
      content: danger
        ? `terraform destroy sẽ xoá mọi tài nguyên do workspace "${ws.name}" quản lý. Không hoàn tác được.`
        : `terraform apply -auto-approve sẽ áp thay đổi lên hạ tầng thật${
            file ? ` với var-file ${file}` : ''
          }. Nên chạy Plan xem trước.`,
      okText: danger ? 'Tôi hiểu — Destroy' : 'Apply',
      okButtonProps: { danger: true },
      cancelText: 'Thôi',
      onOk: () => runCmd(command, true),
    })
  }

  if (loading) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  const notReady = ws.status !== 'ready'

  return (
    <div>
      {parseErrors.length > 0 && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="Một số file .tf không parse được"
          description={parseErrors.join(' · ')}
        />
      )}
      <Space wrap style={{ marginBottom: 16 }}>
        <Text strong>File giá trị (.tfvars):</Text>
        <Select
          style={{ minWidth: 220 }}
          placeholder="— chưa chọn —"
          value={file || undefined}
          onChange={changeFile}
          options={Array.from(new Set([...files, ...(file ? [file] : [])])).map((f) => ({
            value: f,
            label: f,
          }))}
          notFoundContent="Chưa có file .tfvars — bấm 📂 Chọn…"
        />
        <Button onClick={() => setPickerOpen(true)}>📂 Chọn…</Button>
        <Button onClick={() => setNewFileOpen(true)}>➕ File mới</Button>
        {ws.subdir && (
          <Tooltip title="Root Terraform trong repo — đổi ở tab Thông tin">
            <Tag color="purple">📁 {ws.subdir}</Tag>
          </Tooltip>
        )}
        {ws.source === 'git' && ws.auto_sync && (
          <Tooltip title="Workspace git: tự git pull --ff-only trước plan/apply/destroy">
            <Tag color="cyan">🔄 tự sync trước khi chạy</Tag>
          </Tooltip>
        )}
      </Space>

      {defs.length === 0 ? (
        <div>
          <Empty description="Không tìm thấy block variable nào trong *.tf của thư mục Terraform hiện tại" />
          {subdirOpts != null && subdirOpts.length > 0 && (
            <Alert
              type="info"
              showIcon
              style={{ marginTop: 16, maxWidth: 640, marginLeft: 'auto', marginRight: 'auto' }}
              message="Có vẻ Terraform nằm trong thư mục con của repo"
              description={
                <Space wrap style={{ marginTop: 8 }}>
                  <Select
                    style={{ minWidth: 260 }}
                    value={subdirPick}
                    onChange={setSubdirPick}
                    options={subdirOpts.map((s) => ({ value: s, label: `📁 ${s}` }))}
                  />
                  <Button
                    type="primary"
                    loading={subdirSaving}
                    onClick={async () => {
                      setSubdirSaving(true)
                      try {
                        await api.wsPatch(ws.id, { subdir: subdirPick })
                        message.success(`Đã đặt root Terraform: ${subdirPick}`)
                        onChanged()
                        await load()
                      } catch (e) {
                        message.error(String(e))
                      } finally {
                        setSubdirSaving(false)
                      }
                    }}
                  >
                    Dùng thư mục này
                  </Button>
                </Space>
              }
            />
          )}
        </div>
      ) : (
        <Form layout="vertical" style={{ maxWidth: 680 }}>
          {defs.map((d) => {
            const kind = controlKind(d)
            const v = values[d.name]
            const set = (nv: unknown) => setValues((p) => ({ ...p, [d.name]: nv }))
            const label = (
              <Space size={6}>
                <Text code>{d.name}</Text>
                <Tag style={{ fontSize: 11 }}>{d.var_type}</Tag>
                {d.default == null && !d.sensitive && <Tag color="orange">bắt buộc</Tag>}
                {d.sensitive && <Tag color="red">sensitive</Tag>}
              </Space>
            )
            return (
              <Form.Item
                key={d.name}
                label={label}
                extra={d.description || undefined}
                style={{ marginBottom: 14 }}
              >
                {kind === 'bool' ? (
                  <Switch checked={Boolean(v)} onChange={set} />
                ) : kind === 'number' ? (
                  <InputNumber
                    style={{ width: 220 }}
                    value={v as number | null}
                    onChange={set}
                    placeholder={d.default != null ? String(d.default) : ''}
                  />
                ) : kind === 'json' ? (
                  <Input.TextArea
                    autoSize={{ minRows: 2, maxRows: 10 }}
                    style={{ fontFamily: 'monospace' }}
                    value={String(v ?? '')}
                    onChange={(e) => set(e.target.value)}
                    placeholder={d.default != null ? JSON.stringify(d.default) : '[] hoặc {}'}
                  />
                ) : kind === 'secret' ? (
                  <Input.Password
                    value={String(v ?? '')}
                    onChange={(e) => set(e.target.value)}
                    placeholder="(giá trị nhạy cảm — chỉ ghi vào tfvars)"
                  />
                ) : (
                  <Input
                    value={String(v ?? '')}
                    onChange={(e) => set(e.target.value)}
                    placeholder={d.default != null ? String(d.default) : ''}
                  />
                )}
              </Form.Item>
            )
          })}
        </Form>
      )}

      <Space style={{ marginTop: 4, marginBottom: 8 }}>
        <Button type="primary" loading={saving} onClick={save} disabled={defs.length === 0 && !file}>
          💾 Lưu vào {file || 'tfvars'}
        </Button>
        <Checkbox checked={replace} onChange={(e) => setReplace(e.target.checked)}>
          Ghi đè cả file (xoá biến không có trong form)
        </Checkbox>
      </Space>

      <Divider style={{ margin: '16px 0' }} />

      {notReady ? (
        <Alert
          type={ws.status === 'error' ? 'error' : 'info'}
          showIcon
          message={
            ws.status === 'cloning' ? 'Đang clone repo — đợi xong sẽ chạy được' : 'Workspace lỗi'
          }
          description={ws.last_error || undefined}
        />
      ) : (
        <Space wrap>
          <Button loading={running === 'init'} onClick={() => runCmd('init')}>
            Init
          </Button>
          <Button loading={running === 'validate'} onClick={() => runCmd('validate')}>
            Validate
          </Button>
          <Button type="primary" loading={running === 'plan'} onClick={() => runCmd('plan')}>
            ▶ Plan
          </Button>
          <Button
            type="primary"
            danger
            loading={running === 'apply'}
            onClick={() => confirmRun('apply')}
          >
            🚀 Apply
          </Button>
          <Button danger loading={running === 'destroy'} onClick={() => confirmRun('destroy')}>
            Destroy
          </Button>
          <Button loading={running === 'output'} onClick={() => runCmd('output')}>
            Output
          </Button>
        </Space>
      )}

      <TfvarsPickerModal
        wsId={ws.id}
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        onPicked={async (rel) => {
          try {
            await api.wsPatch(ws.id, { var_file: rel })
          } catch (e) {
            message.error(String(e))
            return
          }
          if (!files.includes(rel)) setFiles([...files, rel])
          await changeFile(rel)
          onChanged()
        }}
      />

      <Modal
        open={newFileOpen}
        title="Tạo file .tfvars mới"
        okText="Tạo & chọn"
        cancelText="Thôi"
        onCancel={() => setNewFileOpen(false)}
        onOk={() => {
          const name = newFileName.trim()
          if (!name.endsWith('.tfvars') && !name.endsWith('.tfvars.json')) {
            message.error('Tên file phải kết thúc bằng .tfvars hoặc .tfvars.json')
            return
          }
          setNewFileOpen(false)
          changeFile(name)
        }}
      >
        <Input
          value={newFileName}
          onChange={(e) => setNewFileName(e.target.value)}
          placeholder="prod.tfvars"
          onPressEnter={(e) => (e.target as HTMLInputElement).blur()}
        />
        <Text type="secondary" style={{ fontSize: 12 }}>
          File sẽ được tạo trong thư mục workspace khi bấm Lưu.
        </Text>
      </Modal>
    </div>
  )
}
