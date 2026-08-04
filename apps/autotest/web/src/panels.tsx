import { useEffect, useState } from 'react'
import {
  Alert, Button, Card, Col, Collapse, Drawer, Empty, Flex, Form, Input, InputNumber,
  List, Modal, Popconfirm, Row, Select, Space, Switch, Table, Tag, Typography, message,
} from 'antd'
import {
  CaretRightOutlined, DeleteOutlined, EditOutlined, PlusOutlined, ReloadOutlined, RobotOutlined,
} from '@ant-design/icons'
import {
  api, fmtDuration, fmtTime, statusColor, statusLabel,
  type Case, type Env, type Run, type RunResult, type Schedule, type Suite,
} from './api'

const { Text, Paragraph } = Typography

const kindTag = (k: string) => (
  <Tag color={k === 'http' ? 'cyan' : k === 'script' ? 'purple' : 'gold'}>{k}</Tag>
)

/** Textarea JSON có validate — parse lỗi thì báo ngay thay vì để backend từ chối. */
function jsonField(label: string, name: string, placeholder: string, rows = 4) {
  return (
    <Form.Item
      label={label}
      name={name}
      rules={[
        {
          validator: (_, v: string) =>
            !v || !v.trim()
              ? Promise.resolve()
              : (() => {
                  try {
                    JSON.parse(v)
                    return Promise.resolve()
                  } catch (e: any) {
                    return Promise.reject(new Error(`JSON không hợp lệ: ${e.message}`))
                  }
                })(),
        },
      ]}
    >
      <Input.TextArea rows={rows} placeholder={placeholder} style={{ fontFamily: 'monospace', fontSize: 12 }} />
    </Form.Item>
  )
}

const CONFIG_PLACEHOLDER: Record<string, string> = {
  http: '{"method":"GET","url":"{{base_url}}/health","headers":{},"body":""}',
  script: '{"command":"npm test","cwd":"","env":{}}',
  web: '{"steps":[{"action":"navigate","url":"{{base_url}}"},{"action":"act","instruction":"bấm nút Đăng nhập"}]}',
}
const ASSERT_PLACEHOLDER: Record<string, string> = {
  http: '[{"type":"status","value":200},{"type":"json","path":"data.ok","op":"exists"}]',
  script: '[{"type":"exit_code","value":0},{"type":"stdout_contains","value":"PASS"}]',
  web: '[{"type":"text_contains","value":"Đăng xuất"},{"type":"url_contains","value":"/dashboard"}]',
}

// ---------------- Suites ----------------

export function SuitesTab({ onChange, goRuns }: { onChange: () => void; goRuns: () => void }) {
  const [suites, setSuites] = useState<Suite[]>([])
  const [envs, setEnvs] = useState<Env[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [detail, setDetail] = useState<Suite | null>(null)
  const [suiteModal, setSuiteModal] = useState(false)
  const [caseModal, setCaseModal] = useState<{ edit: Case | null } | null>(null)
  const [aiModal, setAiModal] = useState(false)
  const [aiBusy, setAiBusy] = useState(false)
  const [suiteForm] = Form.useForm()
  const [caseForm] = Form.useForm()
  const [aiForm] = Form.useForm()
  const caseKind = Form.useWatch('kind', caseForm) ?? 'http'

  const refresh = () => {
    api.suites().then((list) => {
      setSuites(list)
      if (selected == null && list.length) setSelected(list[0].id)
    })
    api.envs().then(setEnvs)
  }
  const refreshDetail = (id: number | null) => {
    if (id == null) return setDetail(null)
    api.suite(id).then((r) => setDetail(r.ok ? r.suite : null))
  }
  useEffect(refresh, [])
  useEffect(() => refreshDetail(selected), [selected])

  const saveSuite = async () => {
    const v = await suiteForm.validateFields()
    const r = await api.addSuite({ name: v.name, description: v.description ?? '', env_id: v.env_id ?? null })
    if (!r.ok) return message.error(r.error)
    message.success('Đã tạo suite')
    setSuiteModal(false)
    suiteForm.resetFields()
    refresh()
    setSelected(r.suite?.id ?? null)
    onChange()
  }

  const openCaseModal = (edit: Case | null) => {
    setCaseModal({ edit })
    if (edit) {
      caseForm.setFieldsValue({
        name: edit.name,
        kind: edit.kind,
        timeout_ms: edit.timeout_ms,
        enabled: edit.enabled,
        position: edit.position,
        config: JSON.stringify(edit.config, null, 2),
        assertions: JSON.stringify(edit.assertions, null, 2),
        extract: JSON.stringify(edit.extract, null, 2),
      })
    } else {
      caseForm.resetFields()
      caseForm.setFieldsValue({ kind: 'http', timeout_ms: 30000, enabled: true })
    }
  }

  const saveCase = async () => {
    if (!detail) return
    const v = await caseForm.validateFields()
    const body: any = {
      name: v.name,
      kind: v.kind,
      timeout_ms: v.timeout_ms,
      enabled: v.enabled,
      position: v.position,
      config: v.config?.trim() ? JSON.parse(v.config) : {},
      assertions: v.assertions?.trim() ? JSON.parse(v.assertions) : [],
      extract: v.extract?.trim() ? JSON.parse(v.extract) : [],
    }
    const r = caseModal?.edit
      ? await api.updateCase(caseModal.edit.id, body)
      : await api.addCase({ ...body, suite_id: detail.id })
    if (!r.ok) return message.error(r.error)
    message.success('Đã lưu case')
    setCaseModal(null)
    refreshDetail(detail.id)
    refresh()
  }

  const runSuite = async (id: number) => {
    const r = await api.runSuite(id)
    if (!r.ok) return message.error(r.error)
    message.success('Đã bắt đầu chạy nền — xem tab Lịch sử chạy')
    onChange()
    goRuns()
  }

  const runOneCase = async (caseId: number) => {
    const hide = message.loading('Đang chạy case…', 0)
    try {
      const r = await api.runCase(caseId)
      hide()
      if (!r.ok) return message.error(r.error)
      const run: Run = r.run
      const res = run.results?.[0]
      if (run.status === 'pass') message.success(`PASS (${fmtDuration(res?.duration_ms ?? 0)})`)
      else message.warning(`${statusLabel[run.status] ?? run.status}: ${res?.error || 'xem Lịch sử chạy'}`)
      refreshDetail(detail?.id ?? null)
    } catch {
      hide()
    }
  }

  const aiGenerate = async () => {
    if (!detail) return
    const v = await aiForm.validateFields()
    setAiBusy(true)
    try {
      const r = await api.aiGenerate({ suite_id: detail.id, description: v.description, apply: true })
      if (!r.ok) return message.error(r.error)
      const rejected = (r.rejected ?? []).length
      message.success(`AI (${r.model}) đã thêm ${r.cases.length} case${rejected ? `, từ chối ${rejected}` : ''}`)
      setAiModal(false)
      aiForm.resetFields()
      refreshDetail(detail.id)
      refresh()
    } finally {
      setAiBusy(false)
    }
  }

  const caseColumns = [
    { title: '#', dataIndex: 'position', width: 50 },
    { title: 'Tên', dataIndex: 'name' },
    { title: 'Loại', dataIndex: 'kind', width: 80, render: kindTag },
    {
      title: 'Assertion', width: 90,
      render: (_: any, c: Case) => <Text type="secondary">{c.assertions.length}</Text>,
    },
    {
      title: 'Bật', dataIndex: 'enabled', width: 70,
      render: (v: boolean, c: Case) => (
        <Switch
          size="small"
          checked={v}
          onChange={async (checked) => {
            await api.updateCase(c.id, { enabled: checked })
            refreshDetail(detail?.id ?? null)
          }}
        />
      ),
    },
    {
      title: '', width: 150,
      render: (_: any, c: Case) => (
        <Space>
          <Button size="small" icon={<CaretRightOutlined />} onClick={() => runOneCase(c.id)} title="Chạy case này" />
          <Button size="small" icon={<EditOutlined />} onClick={() => openCaseModal(c)} />
          <Popconfirm title="Xoá case này?" onConfirm={async () => { await api.deleteCase(c.id); refreshDetail(detail!.id); refresh() }}>
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <Row gutter={16}>
      <Col span={7}>
        <Card
          size="small"
          title="Bộ kiểm thử"
          extra={<Button size="small" icon={<PlusOutlined />} onClick={() => setSuiteModal(true)}>Tạo</Button>}
        >
          <List
            size="small"
            dataSource={suites}
            locale={{ emptyText: 'Chưa có suite — bấm Tạo' }}
            renderItem={(s) => (
              <List.Item
                onClick={() => setSelected(s.id)}
                style={{ cursor: 'pointer', background: selected === s.id ? 'rgba(34,211,238,0.08)' : undefined, borderRadius: 8, padding: '8px 10px' }}
              >
                <Flex vertical style={{ width: '100%' }} gap={2}>
                  <Flex justify="space-between">
                    <Text strong>{s.name}</Text>
                    {s.last_run_status && <Tag color={statusColor[s.last_run_status]}>{statusLabel[s.last_run_status] ?? s.last_run_status}</Tag>}
                  </Flex>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {s.case_count} case · chạy gần nhất {fmtTime(s.last_run_at)}
                  </Text>
                </Flex>
              </List.Item>
            )}
          />
        </Card>
      </Col>
      <Col span={17}>
        {!detail ? (
          <Empty description="Chọn một suite bên trái" />
        ) : (
          <Card
            size="small"
            title={
              <Space>
                <Text strong>{detail.name}</Text>
                {detail.description && <Text type="secondary">— {detail.description}</Text>}
              </Space>
            }
            extra={
              <Space>
                <Select
                  size="small"
                  style={{ width: 170 }}
                  placeholder="Environment mặc định"
                  value={detail.env_id ?? undefined}
                  allowClear
                  options={envs.map((e) => ({ value: e.id, label: e.name }))}
                  onChange={async (v) => {
                    await api.updateSuite(detail.id, { env_id: v ?? 0 })
                    refreshDetail(detail.id)
                  }}
                />
                <Button size="small" icon={<RobotOutlined />} onClick={() => setAiModal(true)}>AI sinh case</Button>
                <Button size="small" icon={<PlusOutlined />} onClick={() => openCaseModal(null)}>Thêm case</Button>
                <Button size="small" type="primary" icon={<CaretRightOutlined />} onClick={() => runSuite(detail.id)}>
                  Chạy suite
                </Button>
                <Popconfirm title="Xoá suite + toàn bộ case và lịch sử?" onConfirm={async () => {
                  await api.deleteSuite(detail.id)
                  setSelected(null); setDetail(null); refresh(); onChange()
                }}>
                  <Button size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              </Space>
            }
          >
            <Table size="small" rowKey="id" pagination={false} dataSource={detail.cases ?? []} columns={caseColumns} />
          </Card>
        )}
      </Col>

      <Modal title="Tạo bộ kiểm thử" open={suiteModal} onOk={saveSuite} onCancel={() => setSuiteModal(false)} okText="Tạo" cancelText="Hủy">
        <Form form={suiteForm} layout="vertical">
          <Form.Item label="Tên" name="name" rules={[{ required: true, message: 'Nhập tên suite' }]}>
            <Input placeholder="VD: Smoke API chính" />
          </Form.Item>
          <Form.Item label="Mô tả" name="description">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item label="Environment mặc định" name="env_id">
            <Select allowClear options={envs.map((e) => ({ value: e.id, label: e.name }))} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={caseModal?.edit ? `Sửa case #${caseModal.edit.id}` : 'Thêm test case'}
        open={!!caseModal}
        onOk={saveCase}
        onCancel={() => setCaseModal(null)}
        okText="Lưu"
        cancelText="Hủy"
        width={720}
      >
        <Form form={caseForm} layout="vertical">
          <Row gutter={12}>
            <Col span={10}>
              <Form.Item label="Tên" name="name" rules={[{ required: true, message: 'Nhập tên case' }]}>
                <Input />
              </Form.Item>
            </Col>
            <Col span={5}>
              <Form.Item label="Loại" name="kind" rules={[{ required: true }]}>
                <Select options={[{ value: 'http' }, { value: 'script' }, { value: 'web' }]} />
              </Form.Item>
            </Col>
            <Col span={5}>
              <Form.Item label="Timeout (ms)" name="timeout_ms">
                <InputNumber style={{ width: '100%' }} min={100} max={600000} />
              </Form.Item>
            </Col>
            <Col span={4}>
              <Form.Item label="Bật" name="enabled" valuePropName="checked">
                <Switch />
              </Form.Item>
            </Col>
          </Row>
          {caseKind === 'web' && (
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 12 }}
              message="Case web điều khiển app Mini Browser (port 4360) — app đó phải đang chạy."
            />
          )}
          {jsonField('Config (JSON)', 'config', CONFIG_PLACEHOLDER[caseKind], 5)}
          {jsonField('Assertions (mảng JSON)', 'assertions', ASSERT_PLACEHOLDER[caseKind], 4)}
          {jsonField('Extract — trích biến cho case sau (mảng JSON)', 'extract', '[{"var":"token","from":"json","path":"data.token"}]', 2)}
        </Form>
      </Modal>

      <Modal
        title="AI sinh test case"
        open={aiModal}
        onOk={aiGenerate}
        onCancel={() => setAiModal(false)}
        okText="Sinh & thêm vào suite"
        cancelText="Hủy"
        confirmLoading={aiBusy}
        width={680}
      >
        <Form form={aiForm} layout="vertical">
          <Form.Item
            label="Mô tả cần test (tính năng, OpenAPI, curl mẫu…)"
            name="description"
            rules={[{ required: true, message: 'Nhập mô tả' }]}
          >
            <Input.TextArea
              rows={8}
              placeholder={'VD: API đăng nhập POST {{base_url}}/api/login nhận {"email","password"}, trả 200 + data.token; sai mật khẩu trả 401. Sinh test happy path + sai mật khẩu + thiếu trường.'}
            />
          </Form.Item>
          <Text type="secondary">AI thấy các biến environment sẵn có và sẽ dùng {'{{var}}'} thay vì hard-code.</Text>
        </Form>
      </Modal>
    </Row>
  )
}

// ---------------- Runs ----------------

function ResultBlock({ r }: { r: RunResult }) {
  return (
    <Flex vertical gap={8}>
      {r.error && <Alert type="error" showIcon message={r.error} />}
      {r.assertions.length > 0 && (
        <List
          size="small"
          dataSource={r.assertions}
          renderItem={(a) => (
            <List.Item style={{ padding: '4px 0' }}>
              <Space align="start">
                <Text style={{ color: a.pass ? '#22c55e' : '#ef4444' }}>{a.pass ? '✓' : '✗'}</Text>
                <Flex vertical>
                  <Text>{a.desc}</Text>
                  {!a.pass && (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      thực tế: {a.actual || '(rỗng)'}
                    </Text>
                  )}
                </Flex>
              </Space>
            </List.Item>
          )}
        />
      )}
      {r.log && <pre className="log">{r.log}</pre>}
    </Flex>
  )
}

export function RunsTab() {
  const [runs, setRuns] = useState<Run[]>([])
  const [detail, setDetail] = useState<Run | null>(null)
  const [diagnosis, setDiagnosis] = useState<string | null>(null)
  const [diagBusy, setDiagBusy] = useState(false)

  const refresh = () => api.runs(null, 80).then(setRuns)
  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 5000)
    return () => clearInterval(t)
  }, [])

  const openDetail = (id: number) => {
    setDiagnosis(null)
    api.run(id).then((r) => r.ok && setDetail(r.run))
  }

  const diagnose = async () => {
    if (!detail) return
    setDiagBusy(true)
    try {
      const r = await api.aiDiagnose({ run_id: detail.id })
      if (!r.ok) return message.error(r.error)
      setDiagnosis(r.analysis)
    } finally {
      setDiagBusy(false)
    }
  }

  return (
    <>
      <Card
        size="small"
        title="Lịch sử chạy"
        extra={<Button size="small" icon={<ReloadOutlined />} onClick={refresh} />}
      >
        <Table
          size="small"
          rowKey="id"
          dataSource={runs}
          pagination={{ pageSize: 20 }}
          onRow={(r) => ({ onClick: () => openDetail(r.id), style: { cursor: 'pointer' } })}
          columns={[
            { title: '#', dataIndex: 'id', width: 60 },
            { title: 'Đối tượng', dataIndex: 'target' },
            {
              title: 'Trạng thái', dataIndex: 'status', width: 110,
              render: (s: string) => <Tag color={statusColor[s]}>{statusLabel[s] ?? s}</Tag>,
            },
            {
              title: 'Kết quả', width: 150,
              render: (_: any, r: Run) =>
                r.status === 'running' ? (
                  <Text type="secondary">…</Text>
                ) : (
                  <Text>
                    <Text style={{ color: '#22c55e' }}>{r.passed}✓</Text>{' '}
                    {r.failed > 0 && <Text style={{ color: '#ef4444' }}>{r.failed}✗ </Text>}
                    {r.errors > 0 && <Text style={{ color: '#f97316' }}>{r.errors}⚠ </Text>}
                    / {r.total}
                  </Text>
                ),
            },
            { title: 'Trigger', dataIndex: 'trigger', width: 90 },
            { title: 'Bắt đầu', dataIndex: 'started_at', width: 170, render: fmtTime },
            {
              title: '', width: 80,
              render: (_: any, r: Run) =>
                r.status === 'running' && (
                  <Button
                    size="small"
                    danger
                    onClick={async (e) => {
                      e.stopPropagation()
                      await api.cancelRun(r.id)
                      message.info('Đã yêu cầu hủy — run dừng sau case hiện tại')
                    }}
                  >
                    Hủy
                  </Button>
                ),
            },
          ]}
        />
      </Card>

      <Drawer
        title={detail ? `Run #${detail.id} — ${detail.target}` : ''}
        open={!!detail}
        onClose={() => setDetail(null)}
        width={780}
        extra={
          detail && detail.status !== 'pass' && detail.status !== 'running' && (
            <Button icon={<RobotOutlined />} loading={diagBusy} onClick={diagnose}>
              AI chẩn đoán
            </Button>
          )
        }
      >
        {detail && (
          <Flex vertical gap={12}>
            <Space>
              <Tag color={statusColor[detail.status]}>{statusLabel[detail.status] ?? detail.status}</Tag>
              <Text type="secondary">
                {detail.passed}✓ {detail.failed}✗ {detail.errors}⚠ {detail.skipped} bỏ qua · trigger {detail.trigger} · {fmtTime(detail.started_at)}
              </Text>
            </Space>
            {diagnosis && (
              <Card size="small" title="🤖 AI chẩn đoán">
                <Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 0 }}>{diagnosis}</Paragraph>
              </Card>
            )}
            <Collapse
              items={(detail.results ?? []).map((r) => ({
                key: r.id,
                label: (
                  <Space>
                    <Tag color={statusColor[r.status]}>{statusLabel[r.status] ?? r.status}</Tag>
                    <Text>{r.name}</Text>
                    {kindTag(r.kind)}
                    <Text type="secondary">{fmtDuration(r.duration_ms)}</Text>
                  </Space>
                ),
                children: <ResultBlock r={r} />,
              }))}
            />
          </Flex>
        )}
      </Drawer>
    </>
  )
}

// ---------------- Environments ----------------

export function EnvsTab() {
  const [envs, setEnvs] = useState<Env[]>([])
  const [modal, setModal] = useState<{ edit: Env | null } | null>(null)
  const [form] = Form.useForm()

  const refresh = () => {
    api.envs().then(setEnvs)
  }
  useEffect(refresh, [])

  const open = (edit: Env | null) => {
    setModal({ edit })
    if (edit) form.setFieldsValue({ name: edit.name, vars: JSON.stringify(edit.vars, null, 2) })
    else {
      form.resetFields()
      form.setFieldsValue({ vars: '{\n  "base_url": "http://127.0.0.1:8080"\n}' })
    }
  }

  const save = async () => {
    const v = await form.validateFields()
    const r = await api.setEnv({ name: v.name, vars: JSON.parse(v.vars || '{}') })
    if (!r.ok) return message.error(r.error)
    message.success('Đã lưu environment')
    setModal(null)
    refresh()
  }

  return (
    <Card
      size="small"
      title="Environment — bộ biến {{var}} cho test"
      extra={<Button size="small" icon={<PlusOutlined />} onClick={() => open(null)}>Thêm</Button>}
    >
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        dataSource={envs}
        locale={{ emptyText: 'Chưa có environment. Tạo một cái với base_url để test dùng {{base_url}}.' }}
        columns={[
          { title: 'Tên', dataIndex: 'name', width: 180 },
          {
            title: 'Biến',
            dataIndex: 'vars',
            render: (vars: Record<string, string>) => (
              <Space wrap>
                {Object.entries(vars).map(([k, v]) => (
                  <Tag key={k}>
                    {k} = {String(v).length > 40 ? String(v).slice(0, 40) + '…' : String(v)}
                  </Tag>
                ))}
              </Space>
            ),
          },
          {
            title: '', width: 110,
            render: (_: any, e: Env) => (
              <Space>
                <Button size="small" icon={<EditOutlined />} onClick={() => open(e)} />
                <Popconfirm title="Xoá environment này?" onConfirm={async () => { await api.deleteEnv(e.id); refresh() }}>
                  <Button size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
      <Modal
        title={modal?.edit ? 'Sửa environment' : 'Thêm environment'}
        open={!!modal}
        onOk={save}
        onCancel={() => setModal(null)}
        okText="Lưu"
        cancelText="Hủy"
      >
        <Form form={form} layout="vertical">
          <Form.Item label="Tên" name="name" rules={[{ required: true, message: 'Nhập tên' }]}>
            <Input placeholder="VD: staging" disabled={!!modal?.edit} />
          </Form.Item>
          {jsonField('Biến (JSON object)', 'vars', '{"base_url":"http://…","token":"…"}', 8)}
        </Form>
      </Modal>
    </Card>
  )
}

// ---------------- Schedules ----------------

export function SchedulesTab() {
  const [schedules, setSchedules] = useState<Schedule[]>([])
  const [suites, setSuites] = useState<Suite[]>([])
  const [browserUrl, setBrowserUrl] = useState('')
  const [form] = Form.useForm()

  const refresh = () => {
    api.schedules().then(setSchedules)
    api.suites().then(setSuites)
    api.settings().then((s) => setBrowserUrl(s.browser_url))
  }
  useEffect(refresh, [])

  const add = async () => {
    const v = await form.validateFields()
    const r = await api.setSchedule({ suite_id: v.suite_id, interval_min: v.interval_min, enabled: true })
    if (!r.ok) return message.error(r.error)
    message.success('Đã đặt lịch')
    form.resetFields()
    refresh()
  }

  return (
    <Flex vertical gap={16}>
      <Card size="small" title="Đặt lịch chạy định kỳ">
        <Form form={form} layout="inline" initialValues={{ interval_min: 60 }}>
          <Form.Item label="Suite" name="suite_id" rules={[{ required: true, message: 'Chọn suite' }]}>
            <Select style={{ width: 240 }} options={suites.map((s) => ({ value: s.id, label: s.name }))} />
          </Form.Item>
          <Form.Item label="Mỗi (phút)" name="interval_min" rules={[{ required: true }]}>
            <InputNumber min={1} max={10080} />
          </Form.Item>
          <Button type="primary" onClick={add}>Lưu lịch</Button>
        </Form>
      </Card>

      <Card size="small" title="Lịch hiện có">
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={schedules}
          locale={{ emptyText: 'Chưa có lịch nào' }}
          columns={[
            { title: 'Suite', dataIndex: 'suite_name' },
            { title: 'Chu kỳ', dataIndex: 'interval_min', width: 120, render: (m: number) => `mỗi ${m} phút` },
            {
              title: 'Bật', dataIndex: 'enabled', width: 80,
              render: (v: boolean, s: Schedule) => (
                <Switch
                  size="small"
                  checked={v}
                  onChange={async (checked) => {
                    await api.setSchedule({ suite_id: s.suite_id, interval_min: s.interval_min, enabled: checked })
                    refresh()
                  }}
                />
              ),
            },
            { title: 'Chạy gần nhất', dataIndex: 'last_run_at', width: 180, render: fmtTime },
            {
              title: '', width: 60,
              render: (_: any, s: Schedule) => (
                <Popconfirm title="Xoá lịch này?" onConfirm={async () => { await api.deleteSchedule(s.suite_id); refresh() }}>
                  <Button size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>

      <Card size="small" title="Cài đặt">
        <Space>
          <Text>URL Mini Browser (test web):</Text>
          <Input
            style={{ width: 280 }}
            placeholder="http://127.0.0.1:4360 (mặc định)"
            value={browserUrl}
            onChange={(e) => setBrowserUrl(e.target.value)}
          />
          <Button
            onClick={async () => {
              await api.saveSettings({ browser_url: browserUrl })
              message.success('Đã lưu')
            }}
          >
            Lưu
          </Button>
        </Space>
      </Card>
    </Flex>
  )
}

// ---------------- Activity ----------------

export function ActivityTab() {
  const [items, setItems] = useState<any[]>([])
  useEffect(() => {
    api.activity().then(setItems)
  }, [])
  return (
    <Card size="small" title="Hoạt động gần đây">
      <List
        size="small"
        dataSource={items}
        locale={{ emptyText: 'Chưa có hoạt động' }}
        renderItem={(a) => (
          <List.Item>
            <Space>
              <Tag>{a.kind}</Tag>
              <Text>{a.text}</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{fmtTime(a.created_at)}</Text>
            </Space>
          </List.Item>
        )}
      />
    </Card>
  )
}
