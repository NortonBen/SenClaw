import { useState } from 'react'
import type { ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  App,
  Button,
  Card,
  DatePicker,
  Drawer,
  Form,
  Input,
  List,
  Modal,
  Segmented,
  Select,
  Space,
  Switch,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import {
  CaretRightOutlined,
  DeleteOutlined,
  EditOutlined,
  HistoryOutlined,
  PlusOutlined,
  RobotOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import {
  backfillFlow,
  createFlow,
  deleteFlow,
  enableFlow,
  generateFlow,
  listFlows,
  runFlow,
  updateFlow,
} from '../api'
import { ApiError } from '../api'
import { errMsg, isUnavailable } from '../util'
import type { FieldError, FlowImpact, FlowView } from '../types'
import { FlowBuilder } from './FlowBuilder'

const SOURCE_MODES = ['full_refresh', 'incremental_append', 'incremental_merge']
const TRANSFORM_KINDS = ['full', 'incremental_by_time']
const EXPORT_MODES = ['full_refresh', 'append', 'upsert']

const SAMPLE = JSON.stringify(
  {
    flow: 'flow_moi',
    sources: [
      { id: 'src1', connection: 'postgres', table: 'public.orders', mode: 'full_refresh' },
    ],
    transforms: [],
    exports: [],
  },
  null,
  2,
)

export function Flows() {
  const { message } = App.useApp()
  const qc = useQueryClient()
  const [editor, setEditor] = useState<{ mode: 'create' | 'edit'; flow?: FlowView } | null>(null)
  const [backfill, setBackfill] = useState<FlowView | null>(null)

  const flows = useQuery({ queryKey: ['flows'], queryFn: listFlows })

  const invalidate = () => qc.invalidateQueries({ queryKey: ['flows'] })

  const run = useMutation({
    mutationFn: (id: string) => runFlow(id),
    onSuccess: (d) => {
      message.success(`Đã tạo run ${d.run_id.slice(0, 8)}`)
      qc.invalidateQueries({ queryKey: ['runs'] })
    },
    onError: (e) => message.error(errMsg(e)),
  })

  const enable = useMutation({
    mutationFn: (v: { id: string; enabled: boolean }) => enableFlow(v.id, v.enabled),
    onSuccess: invalidate,
    onError: (e) => message.error(errMsg(e)),
  })

  const del = useMutation({
    mutationFn: (id: string) => deleteFlow(id),
    onSuccess: () => {
      message.success('Đã xoá flow')
      invalidate()
    },
    onError: (e) => message.error(errMsg(e)),
  })

  return (
    <div>
      <Space style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          Flows
        </Typography.Title>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setEditor({ mode: 'create' })}>
          Tạo flow
        </Button>
        <GenerateButton onCreated={invalidate} />
      </Space>

      <List<FlowView>
        grid={{ gutter: 16, xs: 1, sm: 1, md: 2, lg: 2, xl: 3 }}
        loading={flows.isLoading}
        dataSource={flows.data?.flows ?? []}
        locale={{ emptyText: 'Chưa có flow nào' }}
        renderItem={(f) => (
          <List.Item>
            <Card
              size="small"
              title={
                <Space>
                  {f.name || f.id}
                  <Tag color={f.enabled ? 'green' : 'default'}>{f.enabled ? 'Bật' : 'Tắt'}</Tag>
                </Space>
              }
              extra={
                <Switch
                  size="small"
                  checked={f.enabled}
                  loading={enable.isPending && enable.variables?.id === f.id}
                  onChange={(checked) => enable.mutate({ id: f.id, enabled: checked })}
                />
              }
            >
              <Typography.Paragraph type="secondary" style={{ marginBottom: 8 }}>
                DAG: {f.dag && f.dag.length ? f.dag.join(' → ') : '(không suy được)'}
              </Typography.Paragraph>
              <Space wrap size="small">
                <Button
                  size="small"
                  type="primary"
                  icon={<CaretRightOutlined />}
                  loading={run.isPending && run.variables === f.id}
                  onClick={() => run.mutate(f.id)}
                >
                  Run
                </Button>
                <Button size="small" icon={<EditOutlined />} onClick={() => setEditor({ mode: 'edit', flow: f })}>
                  Sửa
                </Button>
                <BackfillButton flow={f} onOpen={() => setBackfill(f)} />
                <Button
                  size="small"
                  danger
                  icon={<DeleteOutlined />}
                  loading={del.isPending && del.variables === f.id}
                  onClick={() => del.mutate(f.id)}
                />
              </Space>
            </Card>
          </List.Item>
        )}
      />

      {editor && (
        <FlowEditor
          mode={editor.mode}
          flow={editor.flow}
          onClose={() => setEditor(null)}
          onSaved={() => {
            setEditor(null)
            invalidate()
          }}
        />
      )}

      {backfill && (
        <BackfillModal flow={backfill} onClose={() => setBackfill(null)} />
      )}
    </div>
  )
}

// ---- AI generate (404 → disabled + tooltip) ----
function GenerateButton({ onCreated }: { onCreated: () => void }) {
  const { message } = App.useApp()
  const [unavailable, setUnavailable] = useState(false)
  const [open, setOpen] = useState(false)
  const [prompt, setPrompt] = useState('')

  const gen = useMutation({
    mutationFn: (p: string) => generateFlow(p),
    onSuccess: () => {
      message.success('Đã sinh flow')
      setOpen(false)
      setPrompt('')
      onCreated()
    },
    onError: (e) => {
      if (isUnavailable(e)) {
        setUnavailable(true)
        setOpen(false)
        message.info('Tính năng AI sinh flow chưa khả dụng')
      } else {
        message.error(errMsg(e))
      }
    },
  })

  return (
    <>
      <Tooltip title={unavailable ? 'Chưa khả dụng trên bản build này' : ''}>
        <Button
          icon={<RobotOutlined />}
          disabled={unavailable}
          onClick={() => setOpen(true)}
        >
          AI sinh flow
        </Button>
      </Tooltip>
      <Modal
        open={open}
        title="AI sinh flow"
        okText="Sinh"
        confirmLoading={gen.isPending}
        onOk={() => gen.mutate(prompt)}
        onCancel={() => setOpen(false)}
      >
        <Input.TextArea
          rows={4}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Mô tả pipeline mong muốn bằng tiếng Việt…"
        />
      </Modal>
    </>
  )
}

// Backfill có thể chưa khả dụng — nút vẫn hiển thị, modal xử lý 404.
function BackfillButton({ flow, onOpen }: { flow: FlowView; onOpen: () => void }) {
  void flow
  return (
    <Button size="small" icon={<HistoryOutlined />} onClick={onOpen}>
      Backfill
    </Button>
  )
}

function BackfillModal({ flow, onClose }: { flow: FlowView; onClose: () => void }) {
  const { message } = App.useApp()
  const [range, setRange] = useState<[dayjs.Dayjs, dayjs.Dayjs] | null>(null)

  const mut = useMutation({
    mutationFn: () =>
      backfillFlow(flow.id, range![0].toISOString(), range![1].toISOString()),
    onSuccess: () => {
      message.success('Đã kích hoạt backfill')
      onClose()
    },
    onError: (e) => {
      if (isUnavailable(e)) {
        message.info('Backfill chưa khả dụng trên bản build này')
        onClose()
      } else {
        message.error(errMsg(e))
      }
    },
  })

  return (
    <Modal
      open
      title={`Backfill · ${flow.id}`}
      okText="Chạy backfill"
      okButtonProps={{ disabled: !range }}
      confirmLoading={mut.isPending}
      onOk={() => mut.mutate()}
      onCancel={onClose}
    >
      <Typography.Paragraph type="warning">
        Cảnh báo: bước merge / SCD2 sẽ bị bỏ qua trong backfill (Phase 2).
      </Typography.Paragraph>
      <DatePicker.RangePicker
        showTime
        style={{ width: '100%' }}
        onChange={(v) => setRange(v && v[0] && v[1] ? [v[0], v[1]] : null)}
      />
    </Modal>
  )
}

// ---- Editor: JSON thô + biểu mẫu sinh JSON ----
function FlowEditor({
  mode,
  flow,
  onClose,
  onSaved,
}: {
  mode: 'create' | 'edit'
  flow?: FlowView
  onClose: () => void
  onSaved: () => void
}) {
  const { message } = App.useApp()
  const [topMode, setTopMode] = useState<'visual' | 'json'>('visual')
  const [builderKey, setBuilderKey] = useState(0)
  const [tab, setTab] = useState<'json' | 'form'>('json')
  const [text, setText] = useState(
    flow ? JSON.stringify(flow.def, null, 2) : SAMPLE,
  )
  const [jsonErr, setJsonErr] = useState<string | null>(null)
  const [fieldErrs, setFieldErrs] = useState<FieldError[]>([])

  // def hiện tại để nạp vào canvas khi ở chế độ visual.
  let builderDef: unknown = flow?.def
  if (topMode === 'visual') {
    try {
      builderDef = JSON.parse(text)
    } catch {
      builderDef = flow?.def
    }
  }

  function parse(): unknown | null {
    try {
      const v = JSON.parse(text)
      setJsonErr(null)
      return v
    } catch (e) {
      setJsonErr(errMsg(e))
      return null
    }
  }

  const save = useMutation({
    mutationFn: async () => {
      const def = parse()
      if (def === null) throw new Error('JSON không hợp lệ')
      setFieldErrs([])
      if (mode === 'create') {
        return createFlow(def, false)
      }
      // edit: thử không reset trước; 409 kèm impact → confirm.
      try {
        return await updateFlow(flow!.id, def, false)
      } catch (e) {
        if (e instanceof ApiError && e.status === 409 && e.details) {
          const impact = e.details as FlowImpact
          const ok = await confirmReset(impact)
          if (!ok) throw new Error('Đã huỷ')
          return updateFlow(flow!.id, def, true)
        }
        throw e
      }
    },
    onSuccess: () => {
      message.success(mode === 'create' ? 'Đã tạo flow' : 'Đã cập nhật flow')
      onSaved()
    },
    onError: (e) => {
      if (e instanceof ApiError && e.status === 400 && Array.isArray(e.details)) {
        setFieldErrs(e.details as FieldError[])
        message.error('Flow def không hợp lệ — xem lỗi bên dưới')
      } else if (errMsg(e) !== 'Đã huỷ') {
        message.error(errMsg(e))
      }
    },
  })

  return (
    <Drawer
      title={mode === 'create' ? 'Tạo flow' : `Sửa flow · ${flow?.id}`}
      width={topMode === 'visual' ? '94vw' : 760}
      open
      onClose={onClose}
      extra={
        <Space>
          <Segmented
            value={topMode}
            onChange={(v) => setTopMode(v as 'visual' | 'json')}
            options={[
              { label: 'Kéo-thả', value: 'visual' },
              { label: 'JSON (nâng cao)', value: 'json' },
            ]}
          />
          {topMode === 'json' && (
            <Button type="primary" loading={save.isPending} onClick={() => save.mutate()}>
              Lưu
            </Button>
          )}
        </Space>
      }
    >
      {topMode === 'visual' ? (
        <FlowBuilder
          key={builderKey}
          mode={mode}
          flowId={flow?.id}
          initialDef={builderDef}
          onSaved={onSaved}
          onExportJson={(def) => {
            setText(JSON.stringify(def, null, 2))
            setTopMode('json')
            message.info('Đã chép sang JSON nâng cao')
          }}
        />
      ) : (
        <>
      <Segmented
        value={tab}
        onChange={(v) => setTab(v as 'json' | 'form')}
        options={[
          { label: 'JSON', value: 'json' },
          { label: 'Biểu mẫu', value: 'form' },
        ]}
        style={{ marginBottom: 12 }}
      />
      <Button
        style={{ marginLeft: 8 }}
        onClick={() => {
          try {
            JSON.parse(text)
            setBuilderKey((k) => k + 1)
            setTopMode('visual')
          } catch (e) {
            message.error(`JSON không hợp lệ: ${errMsg(e)}`)
          }
        }}
      >
        Nạp vào canvas
      </Button>

      {tab === 'form' ? (
        <FormBuilder
          onGenerate={(def) => {
            setText(JSON.stringify(def, null, 2))
            setTab('json')
            message.success('Đã sinh JSON — kiểm tra rồi Lưu')
          }}
          initial={flow?.def}
        />
      ) : (
        <>
          <Input.TextArea
            value={text}
            onChange={(e) => setText(e.target.value)}
            rows={20}
            style={{ fontFamily: 'monospace' }}
            status={jsonErr ? 'error' : undefined}
          />
          <Space style={{ marginTop: 8 }}>
            <Button onClick={parse}>Kiểm tra JSON</Button>
            {jsonErr ? (
              <Typography.Text type="danger">{jsonErr}</Typography.Text>
            ) : (
              <Typography.Text type="secondary">JSON hợp lệ về cú pháp</Typography.Text>
            )}
          </Space>
        </>
      )}

      {fieldErrs.length > 0 && (
        <Card size="small" title="Lỗi validate" style={{ marginTop: 12 }}>
          <List
            size="small"
            dataSource={fieldErrs}
            renderItem={(fe) => (
              <List.Item>
                <Typography.Text code>{fe.field}</Typography.Text>: {fe.message}
              </List.Item>
            )}
          />
        </Card>
      )}
        </>
      )}
    </Drawer>
  )
}

function confirmReset(impact: FlowImpact): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: 'Thay đổi cần reset state',
      content: (
        <div>
          <p>Các bước sau sẽ bị reset watermark/state:</p>
          <p>
            <b>{impact.steps_reset.join(', ') || '—'}</b>
          </p>
          {impact.datasets_orphaned.length > 0 && (
            <p>Dataset mồ côi: {impact.datasets_orphaned.join(', ')}</p>
          )}
        </div>
      ),
      okText: 'Xác nhận reset',
      okButtonProps: { danger: true },
      cancelText: 'Huỷ',
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    })
  })
}

interface FormSource {
  id: string
  connection: string
  table?: string
  mode: string
  cursorColumn?: string
  targetNs?: string
  targetDataset?: string
}
interface FormTransform {
  id: string
  kind: string
  sql: string
}
interface FormExport {
  id: string
  connection?: string
  table?: string
  mode: string
}

function FormBuilder({
  onGenerate,
  initial,
}: {
  onGenerate: (def: unknown) => void
  initial?: unknown
}) {
  const init = initial as
    | { flow?: string; sources?: FormSource[]; transforms?: FormTransform[]; exports?: FormExport[] }
    | undefined
  const [form] = Form.useForm()

  function build(values: {
    flow: string
    sources?: FormSource[]
    transforms?: FormTransform[]
    exports?: FormExport[]
  }) {
    const def: Record<string, unknown> = { flow: values.flow }
    def.sources = (values.sources ?? []).map((s) => {
      const src: Record<string, unknown> = {
        id: s.id,
        connection: s.connection,
        mode: s.mode,
      }
      if (s.table) src.table = s.table
      if (s.cursorColumn) src.cursor = { column: s.cursorColumn }
      if (s.targetNs || s.targetDataset) {
        src.target = { namespace: s.targetNs, dataset: s.targetDataset }
      }
      return src
    })
    def.transforms = (values.transforms ?? []).map((t) => ({
      id: t.id,
      kind: t.kind,
      sql: t.sql,
    }))
    def.exports = (values.exports ?? []).map((e) => {
      const ex: Record<string, unknown> = { id: e.id, mode: e.mode }
      if (e.connection) ex.connection = e.connection
      if (e.table) ex.table = e.table
      return ex
    })
    onGenerate(def)
  }

  return (
    <Form
      form={form}
      layout="vertical"
      initialValues={{
        flow: init?.flow ?? 'flow_moi',
        sources: init?.sources ?? [{ id: 'src1', connection: 'postgres', mode: 'full_refresh' }],
        transforms: init?.transforms ?? [],
        exports: init?.exports ?? [],
      }}
      onFinish={build}
    >
      <Form.Item name="flow" label="Tên flow (id)" rules={[{ required: true }]}>
        <Input placeholder="flow_moi" />
      </Form.Item>

      <SectionList
        name="sources"
        title="Sources"
        render={(field) => (
          <>
            <Form.Item {...field} name={[field.name, 'id']} label="ID" rules={[{ required: true }]}>
              <Input />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'connection']} label="Connection" rules={[{ required: true }]}>
              <Input placeholder="postgres" />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'table']} label="Bảng nguồn">
              <Input placeholder="public.orders" />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'mode']} label="Mode" rules={[{ required: true }]}>
              <Select options={SOURCE_MODES.map((m) => ({ value: m, label: m }))} />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'cursorColumn']} label="Cursor column (nếu incremental)">
              <Input placeholder="updated_at" />
            </Form.Item>
            <Space>
              <Form.Item {...field} name={[field.name, 'targetNs']} label="Target namespace">
                <Input placeholder="raw" />
              </Form.Item>
              <Form.Item {...field} name={[field.name, 'targetDataset']} label="Target dataset">
                <Input placeholder="orders" />
              </Form.Item>
            </Space>
          </>
        )}
        empty={{ id: '', connection: '', mode: 'full_refresh' }}
      />

      <SectionList
        name="transforms"
        title="Transforms"
        render={(field) => (
          <>
            <Form.Item {...field} name={[field.name, 'id']} label="ID" rules={[{ required: true }]}>
              <Input />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'kind']} label="Kind" rules={[{ required: true }]}>
              <Select options={TRANSFORM_KINDS.map((m) => ({ value: m, label: m }))} />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'sql']} label="SQL" rules={[{ required: true }]}>
              <Input.TextArea rows={3} style={{ fontFamily: 'monospace' }} />
            </Form.Item>
          </>
        )}
        empty={{ id: '', kind: 'full', sql: '' }}
      />

      <SectionList
        name="exports"
        title="Exports"
        render={(field) => (
          <>
            <Form.Item {...field} name={[field.name, 'id']} label="ID" rules={[{ required: true }]}>
              <Input />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'connection']} label="Connection">
              <Input placeholder="postgres" />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'table']} label="Bảng đích">
              <Input />
            </Form.Item>
            <Form.Item {...field} name={[field.name, 'mode']} label="Mode" rules={[{ required: true }]}>
              <Select options={EXPORT_MODES.map((m) => ({ value: m, label: m }))} />
            </Form.Item>
          </>
        )}
        empty={{ id: '', mode: 'full_refresh' }}
      />

      <Button type="primary" htmlType="submit">
        Sinh JSON
      </Button>
    </Form>
  )
}

function SectionList({
  name,
  title,
  render,
  empty,
}: {
  name: string
  title: string
  render: (field: { key: number; name: number }) => ReactNode
  empty: Record<string, unknown>
}) {
  return (
    <Card size="small" title={title} style={{ marginBottom: 12 }}>
      <Form.List name={name}>
        {(fields, { add, remove }) => (
          <>
            {fields.map((field) => (
              <Card
                key={field.key}
                size="small"
                type="inner"
                style={{ marginBottom: 8 }}
                extra={
                  <Button size="small" danger icon={<DeleteOutlined />} onClick={() => remove(field.name)} />
                }
              >
                {render(field)}
              </Card>
            ))}
            <Button type="dashed" block icon={<PlusOutlined />} onClick={() => add(empty)}>
              Thêm
            </Button>
          </>
        )}
      </Form.List>
    </Card>
  )
}
