import { useEffect, useState } from 'react'
import {
  Button,
  Card,
  Checkbox,
  Drawer,
  Empty,
  Flex,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  message,
} from 'antd'
import { AimOutlined, PlusOutlined, ThunderboltOutlined } from '@ant-design/icons'
import {
  api,
  fmtMoney,
  GOAL_KIND_LABELS,
  GOAL_STATUS_LABELS,
  type Goal,
  type Source,
} from './api'

const { Text, Paragraph } = Typography

const moneyInput = {
  style: { width: '100%' },
  formatter: (v: any) => `${v}`.replace(/\B(?=(\d{3})+(?!\d))/g, ','),
  parser: (v: any) => `${v}`.replace(/,/g, '') as any,
}

function statusTag(status: string) {
  const s = GOAL_STATUS_LABELS[status] ?? { label: status, color: 'default' }
  return <Tag color={s.color}>{s.label}</Tag>
}

export default function GoalsTab({ onChange }: { onChange: () => void }) {
  const [goals, setGoals] = useState<Goal[] | null>(null)
  const [sources, setSources] = useState<Source[]>([])
  const [adding, setAdding] = useState(false)
  const [detail, setDetail] = useState<Goal | null>(null)
  const [form] = Form.useForm()
  const kind = Form.useWatch('kind', form)

  const load = () =>
    api
      .goals()
      .then((r) => {
        setGoals(r.goals)
        if (detail) setDetail(r.goals.find((g) => g.id === detail.id) ?? null)
      })
      .catch(() => {})
  useEffect(() => {
    load()
    api.sources('active').then((r) => setSources(r.sources)).catch(() => {})
  }, [])

  const add = async (v: any) => {
    const r = await api.goalAdd({
      name: v.name,
      kind: v.kind,
      target_amount: Number(v.target_amount || 0),
      deadline: v.deadline || '',
      source_id: v.source_id || undefined,
      note: v.note || '',
    })
    if (r.error) message.error(r.error)
    else {
      message.success('Đã tạo mục tiêu — tiến độ sẽ tự đo từ sổ cái')
      setAdding(false)
      form.resetFields()
      load()
      onChange()
    }
  }

  const finish = async (g: Goal, status: 'done' | 'cancelled') => {
    const r = await api.goalUpdate(g.id, { status })
    if (r.error) message.error(r.error)
    load()
  }

  if (goals === null) return <Spin style={{ display: 'block', margin: '48px auto' }} />

  const active = goals.filter((g) => g.status === 'active')
  const closed = goals.filter((g) => g.status !== 'active')

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Flex justify="space-between" align="center">
        <Text type="secondary">
          Mục tiêu đo tự động từ sổ cái — không cần cập nhật tay. Bấm vào mục tiêu để xem kế hoạch.
        </Text>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setAdding(true)}>
          Thêm mục tiêu
        </Button>
      </Flex>

      {!active.length && <Empty description="Chưa có mục tiêu — đặt mục tiêu đầu tiên (giảm dư nợ, tất toán khoản vay, tăng vốn chủ…)" />}
      <List
        grid={{ gutter: 12, xs: 1, md: 2 }}
        dataSource={active}
        renderItem={(g) => (
          <List.Item>
            <Card
              size="small"
              hoverable
              onClick={() => setDetail(g)}
              title={
                <Space>
                  <AimOutlined style={{ color: '#10b981' }} />
                  <Text strong>{g.name}</Text>
                </Space>
              }
              extra={statusTag(g.eval_status)}
            >
              <Space direction="vertical" size={4} style={{ width: '100%' }}>
                <Space size="small" wrap>
                  <Tag>{GOAL_KIND_LABELS[g.kind] ?? g.kind}</Tag>
                  {g.deadline && <Text type="secondary" style={{ fontSize: 12 }}>hạn {g.deadline}</Text>}
                </Space>
                <Progress
                  percent={Math.round(g.progress_pct)}
                  strokeColor={g.eval_status === 'at_risk' || g.eval_status === 'overdue' ? '#f5222d' : '#10b981'}
                />
                <Text type="secondary" style={{ fontSize: 12 }}>
                  Hiện tại {fmtMoney(g.current)} · còn thiếu <b>{fmtMoney(g.remaining)}</b>
                  {g.pace_per_month > 0 && g.months_left >= 1 && (
                    <> · cần ~<b>{fmtMoney(g.pace_per_month)}</b>/tháng để kịp hạn</>
                  )}
                </Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  Tiến độ {Math.round(g.progress_pct)}% / thời gian đã trôi {Math.round(g.elapsed_pct)}% · kế hoạch{' '}
                  {g.steps.filter((s) => s.status === 'done').length}/{g.steps.length} bước
                </Text>
              </Space>
            </Card>
          </List.Item>
        )}
      />

      {closed.length > 0 && (
        <Card title="Đã đóng" size="small">
          <List
            size="small"
            dataSource={closed}
            renderItem={(g) => (
              <List.Item>
                <Space>
                  {statusTag(g.status)}
                  <Text>{g.name}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>{GOAL_KIND_LABELS[g.kind] ?? g.kind}</Text>
                </Space>
              </List.Item>
            )}
          />
        </Card>
      )}

      <Modal
        title="Thêm mục tiêu"
        open={adding}
        onCancel={() => setAdding(false)}
        onOk={() => form.submit()}
        okText="Tạo mục tiêu"
        cancelText="Huỷ"
        destroyOnHidden
      >
        <Form form={form} layout="vertical" onFinish={add} initialValues={{ kind: 'reduce_debt' }}>
          <Form.Item name="name" label="Tên mục tiêu" rules={[{ required: true, message: 'Nhập tên' }]}>
            <Input placeholder="Giảm dư nợ xuống 500 triệu trước Tết…" />
          </Form.Item>
          <Flex gap={12}>
            <Form.Item name="kind" label="Loại" style={{ flex: 1 }} rules={[{ required: true }]}>
              <Select options={Object.entries(GOAL_KIND_LABELS).map(([value, label]) => ({ value, label }))} />
            </Form.Item>
            {kind !== 'payoff_source' && (
              <Form.Item name="target_amount" label="Giá trị đích" style={{ flex: 1 }} rules={[{ required: true, message: 'Nhập đích' }]}>
                <InputNumber min={0} {...moneyInput} />
              </Form.Item>
            )}
          </Flex>
          {(kind === 'payoff_source' || kind === 'reduce_debt') && (
            <Form.Item
              name="source_id"
              label={kind === 'payoff_source' ? 'Khoản vay cần tất toán' : 'Giới hạn 1 nguồn (tuỳ chọn)'}
              rules={kind === 'payoff_source' ? [{ required: true, message: 'Chọn khoản vay' }] : []}
            >
              <Select
                allowClear={kind !== 'payoff_source'}
                options={sources
                  .filter((s) => s.is_debt)
                  .map((s) => ({ value: s.id, label: `${s.name} — dư ${fmtMoney(s.outstanding, s.currency)}` }))}
              />
            </Form.Item>
          )}
          <Form.Item name="deadline" label="Hạn hoàn thành (khuyến nghị — để đánh giá tiến độ)">
            <Input type="date" />
          </Form.Item>
          <Form.Item name="note" label="Ghi chú">
            <Input />
          </Form.Item>
        </Form>
      </Modal>

      <GoalDrawer goal={detail} onClose={() => setDetail(null)} onChanged={load} onFinish={finish} />
    </Space>
  )
}

function GoalDrawer({
  goal,
  onClose,
  onChanged,
  onFinish,
}: {
  goal: Goal | null
  onClose: () => void
  onChanged: () => void
  onFinish: (g: Goal, status: 'done' | 'cancelled') => void
}) {
  const [planning, setPlanning] = useState(false)
  const [stepForm] = Form.useForm()

  const plan = async (ai: boolean) => {
    if (!goal) return
    setPlanning(true)
    try {
      const r = await api.goalPlan(goal.id, ai)
      if (r.error) message.error(r.error)
      else message.success(r.source === 'ai' ? `AI đã lên kế hoạch ${r.steps?.length ?? 0} bước` : `Đã chia mốc tự động ${r.steps?.length ?? 0} bước`)
      onChanged()
    } finally {
      setPlanning(false)
    }
  }

  const stepAction = async (action: 'done' | 'todo' | 'delete', stepId: number) => {
    if (!goal) return
    const r = await api.goalStep(goal.id, { action, step_id: stepId })
    if (r.error) message.error(r.error)
    onChanged()
  }

  const addStep = async (v: any) => {
    if (!goal) return
    const r = await api.goalStep(goal.id, {
      action: 'add',
      title: v.title,
      due_date: v.due_date || '',
      amount: Number(v.amount || 0),
    })
    if (r.error) message.error(r.error)
    else stepForm.resetFields()
    onChanged()
  }

  return (
    <Drawer title={goal?.name ?? ''} open={goal !== null} onClose={onClose} width={640}>
      {goal && (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Card size="small">
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Space wrap>
                {statusTag(goal.eval_status)}
                <Tag>{GOAL_KIND_LABELS[goal.kind] ?? goal.kind}</Tag>
                {goal.deadline && <Text type="secondary">hạn {goal.deadline}</Text>}
              </Space>
              <Progress percent={Math.round(goal.progress_pct)} strokeColor="#10b981" />
              <Text>
                {fmtMoney(goal.current)} hiện tại → đích {fmtMoney(goal.target_amount)} · còn thiếu{' '}
                <b>{fmtMoney(goal.remaining)}</b>
              </Text>
              {goal.pace_per_month > 0 && goal.months_left >= 1 && (
                <Text type="secondary">Cần ~{fmtMoney(goal.pace_per_month)}/tháng trong {Math.round(goal.months_left)} tháng còn lại.</Text>
              )}
              {goal.note && <Paragraph type="secondary" style={{ marginBottom: 0 }}>{goal.note}</Paragraph>}
            </Space>
          </Card>

          <Card
            size="small"
            title={`Kế hoạch (${goal.steps.filter((s) => s.status === 'done').length}/${goal.steps.length} bước)`}
            extra={
              <Space>
                <Button size="small" type="primary" icon={<ThunderboltOutlined />} loading={planning} onClick={() => plan(true)}>
                  AI lên kế hoạch
                </Button>
                <Button size="small" loading={planning} onClick={() => plan(false)}>
                  Chia mốc tự động
                </Button>
              </Space>
            }
          >
            {!goal.steps.length ? (
              <Empty description="Chưa có kế hoạch — để AI soạn hoặc tự thêm bước" image={Empty.PRESENTED_IMAGE_SIMPLE} />
            ) : (
              <List
                size="small"
                dataSource={goal.steps}
                renderItem={(s) => (
                  <List.Item
                    actions={[
                      <Popconfirm key="d" title="Xoá bước này?" onConfirm={() => stepAction('delete', s.id)}>
                        <Button size="small" type="text" danger>Xoá</Button>
                      </Popconfirm>,
                    ]}
                  >
                    <Space align="start">
                      <Checkbox
                        checked={s.status === 'done'}
                        onChange={(e) => stepAction(e.target.checked ? 'done' : 'todo', s.id)}
                      />
                      <Space direction="vertical" size={0}>
                        <Text delete={s.status === 'done'}>{s.title}</Text>
                        <Space size="small">
                          {s.due_date && <Text type="secondary" style={{ fontSize: 12 }}>{s.due_date}</Text>}
                          {s.amount > 0 && <Text type="secondary" style={{ fontSize: 12 }}>{fmtMoney(s.amount)}</Text>}
                          <Tag style={{ fontSize: 10 }}>{s.source === 'ai' ? 'AI' : s.source === 'auto' ? 'tự động' : 'tay'}</Tag>
                        </Space>
                      </Space>
                    </Space>
                  </List.Item>
                )}
              />
            )}
            <Form form={stepForm} layout="inline" onFinish={addStep} style={{ marginTop: 12, rowGap: 8 }}>
              <Form.Item name="title" rules={[{ required: true, message: 'Nhập bước' }]} style={{ flex: 1, minWidth: 180 }}>
                <Input placeholder="Thêm bước thủ công…" />
              </Form.Item>
              <Form.Item name="due_date">
                <Input type="date" style={{ width: 150 }} />
              </Form.Item>
              <Form.Item name="amount">
                <InputNumber min={0} placeholder="số tiền" {...moneyInput} style={{ width: 130 }} />
              </Form.Item>
              <Button htmlType="submit" icon={<PlusOutlined />} />
            </Form>
          </Card>

          <Space>
            <Popconfirm title="Đánh dấu mục tiêu đã hoàn thành?" onConfirm={() => { onFinish(goal, 'done'); onClose() }}>
              <Button type="primary">Hoàn thành</Button>
            </Popconfirm>
            <Popconfirm title="Huỷ mục tiêu này?" onConfirm={() => { onFinish(goal, 'cancelled'); onClose() }}>
              <Button danger>Huỷ mục tiêu</Button>
            </Popconfirm>
          </Space>
        </Space>
      )}
    </Drawer>
  )
}
