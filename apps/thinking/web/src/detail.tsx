import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Col,
  Descriptions,
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Progress,
  Row,
  Select,
  Space,
  Spin,
  Table,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import { BulbOutlined, FileTextOutlined, PlusOutlined, ThunderboltOutlined } from '@ant-design/icons'
import {
  api,
  HAT_COLORS,
  HAT_KEYS,
  HAT_LABELS,
  PRIORITY_LABELS,
  SOLUTION_STATUS_LABELS,
  STATUS_COLORS,
  STATUS_LABELS,
  W_KEYS,
  W_LABELS,
  type Detail,
  type Solution,
} from './api'

const { Text, Paragraph } = Typography

/** Một ô 5W hoặc một mũ: textarea sửa tại chỗ + nút lưu khi có thay đổi. */
function EntryCard({
  title,
  color,
  content,
  source,
  onSave,
}: {
  title: string
  color?: string
  content: string
  source: string
  onSave: (v: string) => Promise<void>
}) {
  const [value, setValue] = useState(content)
  const [saving, setSaving] = useState(false)
  useEffect(() => setValue(content), [content])
  const dirty = value !== content
  return (
    <Card
      size="small"
      title={
        <Space>
          <span style={{ fontSize: 13 }}>{title}</span>
          {source === 'ai' && <Tag color="purple">AI</Tag>}
        </Space>
      }
      style={color ? { borderTop: `3px solid ${color}` } : undefined}
      extra={
        dirty && (
          <Button
            size="small"
            type="primary"
            loading={saving}
            onClick={async () => {
              setSaving(true)
              await onSave(value)
              setSaving(false)
            }}
          >
            Lưu
          </Button>
        )
      }
    >
      <Input.TextArea
        autoSize={{ minRows: 2, maxRows: 8 }}
        placeholder="Chưa phân tích — gõ trực tiếp hoặc để AI điền"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        variant="borderless"
      />
    </Card>
  )
}

export default function ProblemDetail({ id, onClose }: { id: number; onClose: () => void }) {
  const { message, modal } = AntApp.useApp()
  const [detail, setDetail] = useState<Detail | null>(null)
  // Khoá các nút khi một tác vụ AI đang chạy (chạy khá lâu — nhiều lượt gọi model).
  const [busy, setBusy] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [addingSolution, setAddingSolution] = useState(false)
  const [deciding, setDeciding] = useState<Solution | null>(null)
  const [rationale, setRationale] = useState('')
  const [editForm] = Form.useForm()
  const [solutionForm] = Form.useForm()

  const load = useCallback(() => {
    api.problemGet(id).then((d) => {
      if (d.error) {
        message.error(d.error)
        onClose()
      } else setDetail(d)
    })
  }, [id, message, onClose])
  useEffect(load, [load])

  if (!detail) {
    return (
      <Drawer open width={920} onClose={onClose}>
        <Spin style={{ display: 'block', margin: '48px auto' }} />
      </Drawer>
    )
  }
  const p = detail.problem

  /** Chạy một tác vụ AI: khoá nút, báo lỗi nếu backend trả error, reload. */
  const run = async (key: string, fn: () => Promise<{ error?: string; note?: string }>) => {
    setBusy(key)
    try {
      const r = await fn()
      if (r.error) message.error(r.error, 8)
      else if (r.note) message.info(r.note)
      else message.success('Xong')
      load()
    } catch (e) {
      message.error(String(e))
    } finally {
      setBusy(null)
    }
  }

  const saveEntry = (kind: 'w' | 'hat', key: string) => async (v: string) => {
    const r = kind === 'w' ? await api.wSet(id, { [key]: v }) : await api.hatsSet(id, { [key]: v })
    if (r.error) message.error(r.error)
    load()
  }

  const showReport = async () => {
    const r = await api.report(id)
    if (r.error) {
      message.error(r.error)
      return
    }
    modal.info({
      title: 'Báo cáo phân tích',
      width: 760,
      content: <div className="md-block" style={{ maxHeight: '60vh', overflow: 'auto' }}>{r.report}</div>,
      okText: 'Đóng',
    })
  }

  const openEdit = () => {
    editForm.setFieldsValue({
      title: p.title,
      description: p.description,
      context: p.context,
      goal: p.goal,
      priority: p.priority,
      tags: p.tags,
    })
    setEditing(true)
  }

  const saveEdit = async () => {
    const v = await editForm.validateFields()
    const r = await api.problemUpdate(id, v)
    if (r.error) message.error(r.error)
    setEditing(false)
    load()
  }

  const evaluated = detail.solutions.filter((s) => s.evaluation)
  const best = [...evaluated].sort((a, b) => (b.evaluation!.overall ?? 0) - (a.evaluation!.overall ?? 0))[0]
  const chosen = detail.solutions.find((s) => s.id === p.decided_solution_id)

  const solutionCols = [
    {
      title: 'Giải pháp',
      dataIndex: 'title',
      render: (t: string, s: Solution) => (
        <Tooltip title={s.description || undefined}>
          <Space>
            {t}
            {s.source === 'ai' && <Tag color="purple">AI</Tag>}
            {s.status !== 'proposed' && (
              <Tag color={s.status === 'chosen' ? 'green' : 'default'}>{SOLUTION_STATUS_LABELS[s.status]}</Tag>
            )}
          </Space>
        </Tooltip>
      ),
    },
    {
      title: 'Điểm tổng',
      width: 120,
      render: (_: unknown, s: Solution) =>
        s.evaluation ? (
          <Tooltip title={s.evaluation.verdict}>
            <Progress
              percent={s.evaluation.overall}
              size="small"
              status={s.evaluation.overall >= 70 ? 'success' : s.evaluation.overall >= 45 ? 'normal' : 'exception'}
            />
          </Tooltip>
        ) : (
          <Text type="secondary">chưa chấm</Text>
        ),
    },
    {
      title: '🟡 Lợi · ⚫ Rủi · Khả thi · Công',
      width: 190,
      render: (_: unknown, s: Solution) =>
        s.evaluation ? (
          <Text style={{ fontSize: 12 }}>
            {s.evaluation.benefit} · {s.evaluation.risk} · {s.evaluation.feasibility} · {s.evaluation.effort}
          </Text>
        ) : (
          '—'
        ),
    },
    {
      title: '',
      width: 210,
      render: (_: unknown, s: Solution) => (
        <Space size={4}>
          <Button size="small" loading={busy === `eval:${s.id}`} disabled={!!busy} onClick={() => run(`eval:${s.id}`, () => api.solutionEvaluate(s.id))}>
            Chấm AI
          </Button>
          <Button
            size="small"
            type={s.status === 'chosen' ? 'default' : 'primary'}
            ghost
            disabled={!!busy || s.status === 'chosen'}
            onClick={() => {
              setRationale(p.decision || '')
              setDeciding(s)
            }}
          >
            Chọn
          </Button>
          <Popconfirm title="Xoá giải pháp này?" onConfirm={async () => { await api.solutionDelete(s.id); load() }}>
            <Button size="small" danger type="text">Xoá</Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <Drawer
      open
      width={920}
      onClose={onClose}
      title={
        <Space>
          <span>{p.title}</span>
          <Tag color={STATUS_COLORS[p.status]}>{STATUS_LABELS[p.status] ?? p.status}</Tag>
          <Tag>{PRIORITY_LABELS[p.priority]}</Tag>
        </Space>
      }
      extra={
        <Space>
          <Button icon={<FileTextOutlined />} onClick={showReport}>Báo cáo</Button>
          <Button onClick={openEdit}>Sửa</Button>
          <Button
            type="primary"
            icon={<ThunderboltOutlined />}
            loading={busy === 'analyze'}
            disabled={!!busy}
            onClick={() =>
              run('analyze', async () => {
                message.info('Đang chạy trọn gói 5W → 6 mũ → giải pháp → chấm điểm → tổng hợp (có thể mất vài phút)…', 6)
                return api.analyze(id)
              })
            }
          >
            Phân tích toàn diện
          </Button>
        </Space>
      }
    >
      <Descriptions size="small" column={1} style={{ marginBottom: 12 }}>
        {p.description && <Descriptions.Item label="Mô tả">{p.description}</Descriptions.Item>}
        {p.context && <Descriptions.Item label="Bối cảnh">{p.context}</Descriptions.Item>}
        {p.goal && <Descriptions.Item label="Mục tiêu">{p.goal}</Descriptions.Item>}
      </Descriptions>
      <Progress percent={p.completeness} size="small" format={(v) => `Phân tích ${v}%`} style={{ marginBottom: 16 }} />

      {p.status === 'decided' && (
        <Alert
          type="success"
          showIcon
          style={{ marginBottom: 16 }}
          message={chosen ? `Đã chọn: ${chosen.title}` : 'Đã quyết định'}
          description={p.decision || undefined}
        />
      )}

      <Card
        size="small"
        title="Bước 1 · Làm rõ vấn đề — 5W"
        style={{ marginBottom: 16 }}
        extra={
          <Button
            size="small"
            icon={<BulbOutlined />}
            loading={busy === '5w'}
            disabled={!!busy}
            onClick={() => run('5w', () => api.wGenerate(id))}
          >
            AI điền ô trống
          </Button>
        }
      >
        <Row gutter={[8, 8]}>
          {W_KEYS.map((w) => (
            <Col xs={24} md={12} key={w}>
              <EntryCard
                title={W_LABELS[w]}
                content={detail.five_w[w]?.content ?? ''}
                source={detail.five_w[w]?.source ?? ''}
                onSave={saveEntry('w', w)}
              />
            </Col>
          ))}
        </Row>
      </Card>

      <Card
        size="small"
        title="Bước 2 · Sáu góc nhìn — 6 Mũ Tư Duy"
        style={{ marginBottom: 16 }}
        extra={
          <Button
            size="small"
            icon={<BulbOutlined />}
            loading={busy === 'hats'}
            disabled={!!busy}
            onClick={() => run('hats', () => api.hatsGenerate(id))}
          >
            AI đội mũ (ô trống)
          </Button>
        }
      >
        <Row gutter={[8, 8]}>
          {HAT_KEYS.map((h) => (
            <Col xs={24} md={12} key={h}>
              <EntryCard
                title={HAT_LABELS[h]}
                color={HAT_COLORS[h]}
                content={detail.hats[h]?.content ?? ''}
                source={detail.hats[h]?.source ?? ''}
                onSave={saveEntry('hat', h)}
              />
            </Col>
          ))}
        </Row>
      </Card>

      <Card
        size="small"
        title="Bước 3 · Giải pháp & chấm điểm"
        style={{ marginBottom: 16 }}
        extra={
          <Space>
            <Button size="small" icon={<PlusOutlined />} onClick={() => setAddingSolution(true)}>
              Thêm
            </Button>
            <Button
              size="small"
              icon={<BulbOutlined />}
              loading={busy === 'solutions'}
              disabled={!!busy}
              onClick={() => run('solutions', () => api.solutionsGenerate(id))}
            >
              AI đề xuất
            </Button>
          </Space>
        }
      >
        {best && best.status !== 'chosen' && p.status !== 'decided' && (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 8 }}
            message={`Điểm cao nhất: "${best.title}" (${best.evaluation!.overall}/100) — bấm Chọn để chốt, quyết định là của bạn.`}
          />
        )}
        <Table
          rowKey="id"
          size="small"
          pagination={false}
          dataSource={detail.solutions}
          columns={solutionCols}
          expandable={{
            rowExpandable: (s) => !!(s.evaluation?.detail || s.description),
            expandedRowRender: (s) => (
              <div style={{ paddingLeft: 8 }}>
                {s.description && <Paragraph style={{ fontSize: 13 }}>{s.description}</Paragraph>}
                {s.evaluation?.detail && <div className="md-block">{s.evaluation.detail}</div>}
              </div>
            ),
          }}
        />
      </Card>

      {p.synthesis && (
        <Card size="small" title="🔵 Tổng hợp & khuyến nghị (Mũ Xanh Dương)" style={{ marginBottom: 16 }}>
          <div className="md-block">{p.synthesis}</div>
        </Card>
      )}

      <Modal title="Sửa vấn đề" open={editing} onOk={saveEdit} onCancel={() => setEditing(false)} okText="Lưu" cancelText="Huỷ">
        <Form form={editForm} layout="vertical">
          <Form.Item name="title" label="Tiêu đề" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="Mô tả">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="context" label="Bối cảnh">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="goal" label="Mục tiêu">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Space size="large">
            <Form.Item name="priority" label="Ưu tiên">
              <Select style={{ width: 150 }} options={Object.entries(PRIORITY_LABELS).map(([value, label]) => ({ value, label }))} />
            </Form.Item>
            <Form.Item name="tags" label="Tags">
              <Input />
            </Form.Item>
          </Space>
        </Form>
      </Modal>

      <Modal
        title="Thêm giải pháp"
        open={addingSolution}
        onOk={async () => {
          const v = await solutionForm.validateFields()
          const r = await api.solutionAdd(id, v)
          if (r.error) message.error(r.error)
          setAddingSolution(false)
          solutionForm.resetFields()
          load()
        }}
        onCancel={() => setAddingSolution(false)}
        okText="Thêm"
        cancelText="Huỷ"
      >
        <Form form={solutionForm} layout="vertical">
          <Form.Item name="title" label="Giải pháp" rules={[{ required: true, message: 'Nhập tên giải pháp' }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="Cách làm cụ thể">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={deciding ? `Chốt giải pháp: ${deciding.title}` : ''}
        open={!!deciding}
        onOk={async () => {
          if (!deciding) return
          const r = await api.decide(id, deciding.id, rationale)
          if (r.error) message.error(r.error)
          else message.success('Đã chốt quyết định')
          setDeciding(null)
          load()
        }}
        onCancel={() => setDeciding(null)}
        okText="Chốt quyết định"
        cancelText="Huỷ"
      >
        <Input.TextArea
          rows={3}
          placeholder="Lý do chọn (nên tham chiếu điểm số và góc nhìn các mũ)"
          value={rationale}
          onChange={(e) => setRationale(e.target.value)}
        />
      </Modal>
    </Drawer>
  )
}
