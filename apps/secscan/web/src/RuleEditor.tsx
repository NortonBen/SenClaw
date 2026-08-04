import { useState } from 'react'
import {
  Alert, Button, Card, Col, Empty, Form, Input, Modal, Popconfirm, Radio,
  Row, Select, Space, Table, Tag, Typography, message,
} from 'antd'
import { DeleteOutlined, DownloadOutlined, PlusOutlined } from '@ant-design/icons'
import { api, SEV_COLOR, SEV_LABEL, SEV_ORDER, type CustomRule, type ImportReport, type Severity } from './api'

const { Text, Paragraph } = Typography

const TARGETS = [
  { value: 'header', label: 'Header HTTP' },
  { value: 'cookie_attr', label: 'Thuộc tính cookie' },
  { value: 'dns_txt', label: 'Bản ghi TXT' },
]

const OPS = [
  { value: 'present', label: 'phải có mặt' },
  { value: 'absent', label: 'không được có' },
  { value: 'equals', label: 'phải bằng' },
  { value: 'contains', label: 'phải chứa' },
  { value: 'not_contains', label: 'không được chứa' },
  { value: 'regex', label: 'phải khớp biểu thức' },
  { value: 'not_regex', label: 'không được khớp biểu thức' },
]

const NEEDS_VALUE = ['equals', 'contains', 'not_contains', 'regex', 'not_regex']

export function CustomRules({ rules, onChanged }: { rules: CustomRule[]; onChanged: () => void }) {
  const [adding, setAdding] = useState(false)
  const [importing, setImporting] = useState(false)

  return (
    <Card
      size="small"
      title="Luật tự thêm"
      extra={
        <Space>
          <Button size="small" icon={<DownloadOutlined />} onClick={() => setImporting(true)}>
            Nhập bộ luật
          </Button>
          <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
            Thêm luật
          </Button>
        </Space>
      }
      style={{ marginBottom: 16 }}
    >
      <Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 12 }}>
        Luật ở đây <strong>chạy thật</strong> trong mỗi lần quét, không phải ghi chú. Dạng khai
        báo: so khớp trên header, thuộc tính cookie, hoặc bản ghi TXT. Cố ý không có luật kiểu
        script — bộ luật nhập từ ngoài nhiều nhất cũng chỉ tạo được cảnh báo sai, không chạy được mã.
      </Paragraph>

      {rules.length === 0 ? (
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="chưa có luật tự thêm" />
      ) : (
        <Table
          size="small" rowKey="id" pagination={false} dataSource={rules}
          columns={[
            {
              title: 'Luật', dataIndex: 'title',
              render: (v: string, r: CustomRule) => (
                <Space direction="vertical" size={0}>
                  <Text strong style={{ fontSize: 13 }}>{v}</Text>
                  <Text className="muted" style={{ fontFamily: 'ui-monospace, monospace' }}>{r.id}</Text>
                </Space>
              ),
            },
            {
              title: 'Phép kiểm', key: 'check', render: (_: unknown, r: CustomRule) => (
                <Text style={{ fontSize: 12, fontFamily: 'ui-monospace, monospace' }}>
                  {r.check.target}
                  {r.check.name ? ` · ${r.check.name}` : ''}
                  {' · '}
                  {OPS.find((o) => o.value === r.check.op)?.label ?? r.check.op}
                  {r.check.value ? ` "${r.check.value}"` : ''}
                </Text>
              ),
            },
            {
              title: 'Mức', dataIndex: 'severity', width: 110,
              render: (v: Severity) => <Tag color={SEV_COLOR[v]}>{SEV_LABEL[v]}</Tag>,
            },
            {
              title: 'Nguồn', dataIndex: 'source', width: 140,
              render: (v: string) => (
                <Text className="muted">{v === 'manual' ? 'thêm tay' : v}</Text>
              ),
            },
            {
              title: '', width: 40, key: 'x',
              render: (_: unknown, r: CustomRule) => (
                <Popconfirm
                  title={`Xoá luật ${r.id}?`} okText="Xoá" cancelText="Thôi"
                  okButtonProps={{ danger: true }}
                  onConfirm={async () => {
                    const res = await api.removeRule(r.id)
                    if (res.ok) { message.success('đã xoá'); onChanged() }
                    else message.error(res.error ?? 'lỗi')
                  }}
                >
                  <Button size="small" type="text" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              ),
            },
          ]}
        />
      )}

      <AddRuleModal open={adding} onClose={() => setAdding(false)} onDone={onChanged} />
      <ImportModal open={importing} onClose={() => setImporting(false)} onDone={onChanged} />
    </Card>
  )
}

function AddRuleModal({ open, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => void }) {
  const [form] = Form.useForm()
  const [op, setOp] = useState('present')
  const [target, setTarget] = useState('header')
  const [saving, setSaving] = useState(false)

  const submit = async () => {
    const v = await form.validateFields().catch(() => null)
    if (!v) return
    setSaving(true)
    const res = await api.addRule({
      id: v.id.startsWith('custom:') ? v.id : `custom:${v.id}`,
      title: v.title,
      severity: v.severity,
      rationale: v.rationale ?? '',
      fix: v.fix ?? '',
      check: { target: v.target, name: v.name ?? '', op: v.op, value: v.value ?? '' },
    })
    setSaving(false)
    if (!res.ok) { message.error(res.error ?? 'không lưu được'); return }
    message.success('đã thêm luật')
    form.resetFields(); onClose(); onDone()
  }

  return (
    <Modal
      open={open} onCancel={onClose} onOk={submit} confirmLoading={saving}
      title="Thêm luật quét" okText="Lưu" cancelText="Huỷ" width={640}
    >
      <Form
        form={form} layout="vertical" size="small"
        initialValues={{ severity: 'medium', target: 'header', op: 'present' }}
      >
        <Row gutter={12}>
          <Col span={14}>
            <Form.Item
              name="id" label="Mã luật"
              rules={[{ required: true, message: 'cần mã luật' }]}
              extra="tự thêm tiền tố custom: nếu bạn bỏ qua"
            >
              <Input placeholder="custom:x-request-id" />
            </Form.Item>
          </Col>
          <Col span={10}>
            <Form.Item name="severity" label="Mức">
              <Select options={SEV_ORDER.map((s) => ({ value: s, label: SEV_LABEL[s] }))} />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item name="title" label="Tiêu đề" rules={[{ required: true, message: 'cần tiêu đề' }]}>
          <Input placeholder="Thiếu X-Request-Id" />
        </Form.Item>

        <Form.Item name="rationale" label="Vì sao kiểm" extra="hiện trong báo cáo — nên viết rõ">
          <Input.TextArea rows={2} placeholder="Nội bộ quy định mọi phản hồi phải mang mã truy vết." />
        </Form.Item>

        <Form.Item name="fix" label="Cách sửa">
          <Input.TextArea rows={2} placeholder="Thêm X-Request-Id ở tầng reverse proxy." />
        </Form.Item>

        <Card size="small" title="Phép kiểm" styles={{ body: { paddingBottom: 0 } }}>
          <Form.Item name="target" label="Kiểm ở đâu">
            <Radio.Group
              optionType="button" buttonStyle="solid" options={TARGETS}
              onChange={(e) => setTarget(e.target.value)}
            />
          </Form.Item>
          <Row gutter={12}>
            {target !== 'dns_txt' && (
              <Col span={10}>
                <Form.Item
                  name="name"
                  label={target === 'header' ? 'Tên header' : 'Thuộc tính cookie'}
                  rules={[{ required: true, message: 'cần tên' }]}
                >
                  <Input placeholder={target === 'header' ? 'x-request-id' : 'secure'} />
                </Form.Item>
              </Col>
            )}
            <Col span={target === 'dns_txt' ? 12 : 14}>
              <Form.Item name="op" label="Điều kiện">
                <Select options={OPS} onChange={setOp} />
              </Form.Item>
            </Col>
          </Row>
          {NEEDS_VALUE.includes(op) && (
            <Form.Item name="value" label="Giá trị" rules={[{ required: true, message: 'cần giá trị' }]}>
              <Input placeholder={op.includes('regex') ? '\\d+\\.\\d+' : 'no-store'} />
            </Form.Item>
          )}
        </Card>
      </Form>
    </Modal>
  )
}

function ImportModal({ open, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => void }) {
  const [mode, setMode] = useState<'url' | 'json'>('url')
  const [url, setUrl] = useState('')
  const [json, setJson] = useState('')
  const [report, setReport] = useState<ImportReport | null>(null)
  const [busy, setBusy] = useState(false)

  const run = async (apply: boolean) => {
    setBusy(true)
    const r = await api.importRules({
      url: mode === 'url' ? url : undefined,
      json: mode === 'json' ? json : undefined,
      apply,
    })
    setBusy(false)
    if (!r.ok) { message.error(r.error ?? 'nhập thất bại'); return }
    setReport(r)
    if (apply) {
      message.success(`đã thêm ${r.accepted} luật`)
      setReport(null); setUrl(''); setJson(''); onClose(); onDone()
    }
  }

  return (
    <Modal
      open={open} onCancel={() => { setReport(null); onClose() }} footer={null}
      title="Nhập bộ luật từ nguồn khác" width={680}
    >
      <Alert
        type="info" showIcon style={{ marginBottom: 12 }}
        message="Xem trước rồi mới áp dụng"
        description="Nạp luật từ nguồn ngoài sẽ đổi cách quét chấm điểm, nên bước đầu chỉ đọc và kiểm tra — không có gì được lưu cho tới khi bạn bấm áp dụng. Nguồn URL đi qua cùng bộ chặn SSRF như đích quét và chỉ nhận https."
      />

      <Radio.Group
        value={mode} onChange={(e) => { setMode(e.target.value); setReport(null) }}
        optionType="button" buttonStyle="solid" style={{ marginBottom: 12 }}
        options={[{ value: 'url', label: 'Từ URL' }, { value: 'json', label: 'Dán JSON' }]}
      />

      {mode === 'url' ? (
        <Input
          placeholder="https://example.com/secscan-rules.json"
          value={url} onChange={(e) => { setUrl(e.target.value); setReport(null) }}
        />
      ) : (
        <Input.TextArea
          rows={7} style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}
          placeholder={'{ "rules": [\n  { "id": "custom:cache", "title": "…", "severity": "medium",\n    "check": { "target": "header", "name": "cache-control", "op": "contains", "value": "no-store" } }\n] }'}
          value={json} onChange={(e) => { setJson(e.target.value); setReport(null) }}
        />
      )}

      <Space style={{ marginTop: 12 }}>
        <Button onClick={() => void run(false)} loading={busy}>Xem trước</Button>
        {report && report.accepted > 0 && (
          <Button type="primary" onClick={() => void run(true)} loading={busy}>
            Áp dụng {report.accepted} luật
          </Button>
        )}
      </Space>

      {report && (
        <div style={{ marginTop: 16 }}>
          <Text strong>
            Nhận {report.accepted}/{report.total} luật
            {report.rejected.length > 0 && ` · loại ${report.rejected.length}`}
          </Text>
          {report.rules.map((r) => (
            <div key={r.id} className="finding" style={{ borderLeftColor: SEV_COLOR[r.severity], marginTop: 8 }}>
              <h4>{r.title}</h4>
              <div className="fix">
                {r.target}{r.name ? ` · ${r.name}` : ''} · {r.op}{r.value ? ` "${r.value}"` : ''}
              </div>
              <div className="tag-row">
                <Tag color={SEV_COLOR[r.severity]}>{SEV_LABEL[r.severity]}</Tag>
                <Text className="muted" style={{ fontFamily: 'ui-monospace, monospace' }}>{r.id}</Text>
              </div>
            </div>
          ))}
          {report.rejected.length > 0 && (
            <Alert
              style={{ marginTop: 10 }} type="warning"
              message={`${report.rejected.length} luật bị loại`}
              description={
                <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
                  {report.rejected.map((r) => (
                    <li key={r.id}><code>{r.id}</code> — {r.reason}</li>
                  ))}
                </ul>
              }
            />
          )}
        </div>
      )}
    </Modal>
  )
}

/** Chỉnh mức / tắt một luật dựng sẵn. */
export function OverrideControl({
  ruleId, current, onChanged,
}: {
  ruleId: string
  current?: { severity: Severity | null; enabled: boolean; note: string | null }
  onChanged: () => void
}) {
  const [busy, setBusy] = useState(false)

  const save = async (severity: string | null, enabled: boolean) => {
    setBusy(true)
    const r = await api.setOverride({ rule_id: ruleId, severity, enabled })
    setBusy(false)
    if (!r.ok) message.error(r.error ?? 'lỗi')
    else onChanged()
  }

  const enabled = current?.enabled ?? true

  return (
    <Space size={4}>
      <Select
        size="small" style={{ width: 120 }} disabled={busy || !enabled}
        value={current?.severity ?? 'default'}
        onChange={(v) => void save(v === 'default' ? null : v, enabled)}
        options={[
          { value: 'default', label: 'mức mặc định' },
          ...SEV_ORDER.map((s) => ({ value: s, label: SEV_LABEL[s] })),
        ]}
      />
      <Button
        size="small" type="text" danger={enabled} loading={busy}
        onClick={() => void save(current?.severity ?? null, !enabled)}
      >
        {enabled ? 'tắt' : 'bật lại'}
      </Button>
    </Space>
  )
}
