import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  DatePicker,
  Empty,
  Form,
  Input,
  InputNumber,
  List,
  Popconfirm,
  Progress,
  Radio,
  Row,
  Select,
  Space,
  Statistic,
  Tag,
  Timeline,
  Typography,
} from 'antd'
import { CalendarOutlined, DeleteOutlined, ExperimentOutlined } from '@ant-design/icons'
import {
  del,
  get,
  post,
  type Doc,
  type Plan,
  type Preview,
  type Session,
  type Template,
} from './api'

const KIND_LABEL: Record<string, string> = {
  read: 'Đọc',
  flashcard: 'Thẻ',
  review: 'Ôn',
  quiz: 'Trắc nghiệm',
  recall: 'Tự diễn giải',
}

const WEEKDAYS = [
  { v: 1, l: 'T2' },
  { v: 2, l: 'T3' },
  { v: 3, l: 'T4' },
  { v: 4, l: 'T5' },
  { v: 5, l: 'T6' },
  { v: 6, l: 'T7' },
  { v: 7, l: 'CN' },
]

export default function PlansView({ onOpenSession }: { onOpenSession: (id: string) => void }) {
  const { message } = AntApp.useApp()
  const [docs, setDocs] = useState<Doc[]>([])
  const [templates, setTemplates] = useState<Template[]>([])
  const [plans, setPlans] = useState<Plan[]>([])
  const [preview, setPreview] = useState<Preview | null>(null)
  const [busy, setBusy] = useState(false)
  const [open, setOpen] = useState<Plan | null>(null)
  const [form] = Form.useForm()
  // Hooks must run unconditionally and exactly once per render: this used to
  // live inside a `templates.find(...)` predicate below an early return, which
  // is React error #310 and blanked the whole tab.
  const templateKey = Form.useWatch('template', form) ?? 'standard'
  const chosen = templates.find((t) => t.key === templateKey)

  const load = useCallback(() => {
    get<Doc[]>('/docs').then(setDocs).catch(() => {})
    get<Template[]>('/templates').then(setTemplates).catch(() => {})
    get<Plan[]>('/plans').then(setPlans).catch(() => {})
  }, [])
  useEffect(load, [load])

  const body = () => {
    const v = form.getFieldsValue()
    return {
      doc_ids: v.docIds ?? [],
      title: v.title || undefined,
      goal: v.goal || undefined,
      template: v.template ?? 'standard',
      days: v.days,
      min_per_day: v.minPerDay,
      start_date: v.startDate ? v.startDate.format('YYYY-MM-DD') : undefined,
      weekdays: (v.weekdays ?? [1, 2, 3, 4, 5, 6, 7]).join(','),
      slot_hm: v.slotHm || '20:00',
    }
  }

  const doPreview = async () => {
    setBusy(true)
    try {
      setPreview(await post<Preview>('/plans/preview', body()))
    } catch (e: any) {
      message.error(String(e.message ?? e), 8)
    } finally {
      setBusy(false)
    }
  }

  const doCreate = async (sync: boolean) => {
    setBusy(true)
    try {
      const r = await post<any>('/plans', {
        ...body(),
        sync_calendar: sync,
        reminder_min: form.getFieldValue('reminderMin') ?? undefined,
      })
      message.success(`Đã tạo lộ trình ${r.sessions} buổi`)
      if (r.calendarError) message.error(`Lịch: ${r.calendarError}`, 10)
      else if (r.calendar)
        message.success(
          `Lịch: tạo ${r.calendar.created}, cập nhật ${r.calendar.updated}` +
            (r.calendar.failed?.length ? ` · lỗi ${r.calendar.failed.length}` : ''),
        )
      setPreview(null)
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e), 8)
    } finally {
      setBusy(false)
    }
  }

  if (open) return <PlanDetail plan={open} onBack={() => { setOpen(null); load() }} onOpenSession={onOpenSession} />

  return (
    <Row gutter={16}>
      <Col xs={24} lg={10}>
        <Card title="Tạo lộ trình">
          <Form form={form} layout="vertical" initialValues={{ template: 'standard', weekdays: [1, 2, 3, 4, 5, 6, 7], slotHm: '20:00' }}>
            <Form.Item name="docIds" label="Tài liệu" rules={[{ required: true, message: 'Chọn ít nhất một tài liệu' }]}>
              <Select
                mode="multiple"
                placeholder="Chọn tài liệu đã nạp"
                options={docs.map((d) => ({
                  value: d.id,
                  label: `${d.title} (${d.sectionCount} mục)`,
                }))}
              />
            </Form.Item>
            <Form.Item name="goal" label="Mục tiêu">
              <Input placeholder="ví dụ: thi cuối kỳ ngày 20/9" />
            </Form.Item>
            <Form.Item name="template" label="Mẫu lộ trình">
              <Radio.Group
                optionType="button"
                buttonStyle="solid"
                options={templates.map((t) => ({ value: t.key, label: t.label }))}
                onChange={(e) => {
                  const t = templates.find((x) => x.key === e.target.value)
                  if (t) form.setFieldsValue({ days: t.days, minPerDay: t.minPerDay })
                }}
              />
            </Form.Item>
            {chosen?.detail && (
              <Typography.Paragraph type="secondary" style={{ marginTop: -8 }}>
                {chosen.detail} · ôn lại sau {chosen.reviewOffsets.join('/')} ngày
              </Typography.Paragraph>
            )}
            <Row gutter={8}>
              <Col span={12}>
                <Form.Item name="days" label="Số buổi">
                  <InputNumber min={1} max={365} style={{ width: '100%' }} placeholder={String(chosen?.days ?? 30)} />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item name="minPerDay" label="Phút mỗi buổi">
                  <InputNumber min={5} max={480} style={{ width: '100%' }} placeholder={String(chosen?.minPerDay ?? 30)} />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item name="weekdays" label="Học vào">
              <Checkbox.Group options={WEEKDAYS.map((w) => ({ value: w.v, label: w.l }))} />
            </Form.Item>
            <Row gutter={8}>
              <Col span={12}>
                <Form.Item name="startDate" label="Bắt đầu">
                  <DatePicker style={{ width: '100%' }} format="YYYY-MM-DD" />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item name="slotHm" label="Giờ học">
                  <Input placeholder="20:00" />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item name="reminderMin" label="Nhắc trước (phút)">
              <InputNumber min={0} max={1440} style={{ width: '100%' }} placeholder="bỏ trống = chỉ báo đúng giờ" />
            </Form.Item>
            <Space wrap>
              <Button icon={<ExperimentOutlined />} loading={busy} onClick={doPreview}>
                Xem trước
              </Button>
              <Button type="primary" loading={busy} onClick={() => doCreate(false)}>
                Lưu lộ trình
              </Button>
              <Button type="primary" icon={<CalendarOutlined />} loading={busy} onClick={() => doCreate(true)}>
                Lưu + lên lịch
              </Button>
            </Space>
          </Form>
        </Card>
      </Col>

      <Col xs={24} lg={14}>
        {preview && <PreviewPane p={preview} />}
        <Card title="Lộ trình đã lưu" style={{ marginTop: preview ? 16 : 0 }}>
          {plans.length === 0 ? (
            <Empty description="Chưa có lộ trình" />
          ) : (
            <List
              dataSource={plans}
              renderItem={(p) => (
                <List.Item
                  actions={[
                    <a key="o" onClick={() => setOpen(p)}>Chi tiết</a>,
                    <Popconfirm
                      key="d"
                      title="Xoá lộ trình?"
                      description="Gỡ luôn các sự kiện lịch của nó."
                      onConfirm={async () => {
                        const r = await del<{ eventsRemoved: number }>(`/plans/${p.id}`)
                        message.success(`Đã xoá · gỡ ${r.eventsRemoved} sự kiện lịch`)
                        load()
                      }}
                    >
                      <DeleteOutlined />
                    </Popconfirm>,
                  ]}
                >
                  <List.Item.Meta
                    title={p.title}
                    description={
                      <Space wrap>
                        <Tag>{p.startDate}</Tag>
                        <Tag>{p.sessionCount} buổi · {p.minPerDay} phút</Tag>
                        {p.syncedCount > 0 && <Tag color="purple">{p.syncedCount} trên lịch</Tag>}
                        <Progress
                          percent={p.sessionCount ? Math.round((p.doneCount / p.sessionCount) * 100) : 0}
                          size="small"
                          style={{ width: 140 }}
                        />
                      </Space>
                    }
                  />
                </List.Item>
              )}
            />
          )}
        </Card>
      </Col>
    </Row>
  )
}

function PreviewPane({ p }: { p: Preview }) {
  return (
    <Card title="Xem trước (chưa lưu gì)">
      <Row gutter={16} style={{ marginBottom: 12 }}>
        <Col span={8}><Statistic title="Số buổi" value={p.sessions.length} /></Col>
        <Col span={8}><Statistic title="Nội dung cần" value={`${p.totalEstMinutes} phút`} /></Col>
        <Col span={8}><Statistic title="Ngân sách nội dung" value={`${p.contentBudgetMinutes} phút`} /></Col>
      </Row>

      {!p.feasible && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="Không đủ thời gian cho toàn bộ tài liệu"
          description={
            <>
              <div>{p.notes.join(' · ')}</div>
              <b>Chọn một trong ba cách:</b>
              <ul style={{ marginBottom: 8 }}>
                {p.options.map((o, i) => <li key={i}>{o}</li>)}
              </ul>
              <b>Các mục sẽ bị bỏ nếu giữ nguyên nhịp:</b>
              <ul style={{ margin: 0 }}>
                {p.dropped.map((d) => <li key={d.sectionId}>{d.title} ({d.estMinutes} phút)</li>)}
              </ul>
            </>
          }
        />
      )}
      {(p.warnings ?? []).map((w, i) => (
        <Alert key={i} type="info" showIcon style={{ marginBottom: 8 }} message={w} />
      ))}

      <Timeline
        items={p.sessions.slice(0, 40).map((s) => ({
          children: (
            <Space direction="vertical" size={0}>
              <Typography.Text strong>
                {s.date} {s.startHm} · {s.title} ({s.minutes} phút)
              </Typography.Text>
              <Space wrap size={4}>
                {s.items.map((it, i) => (
                  <Tag key={i} color={it.kind === 'read' ? 'blue' : it.kind === 'quiz' ? 'orange' : 'green'}>
                    {KIND_LABEL[it.kind] ?? it.kind}
                    {it.parts > 1 ? ` ${it.part}/${it.parts}` : ''} · {it.estMinutes}′
                  </Tag>
                ))}
              </Space>
            </Space>
          ),
        }))}
      />
      {p.sessions.length > 40 && (
        <Typography.Text type="secondary">… và {p.sessions.length - 40} buổi nữa</Typography.Text>
      )}
    </Card>
  )
}

function PlanDetail({
  plan,
  onBack,
  onOpenSession,
}: {
  plan: Plan
  onBack: () => void
  onOpenSession: (id: string) => void
}) {
  const { message } = AntApp.useApp()
  const [sessions, setSessions] = useState<Session[]>([])
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    get<Session[]>(`/plans/${plan.id}/sessions`).then(setSessions).catch(() => {})
  }, [plan.id])
  useEffect(load, [load])

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Space wrap>
        <Button onClick={onBack}>← Danh sách</Button>
        <Button
          type="primary"
          icon={<CalendarOutlined />}
          loading={busy}
          onClick={async () => {
            setBusy(true)
            try {
              const r = await post<any>(`/plans/${plan.id}/sync`, {})
              message.success(
                `Lịch: tạo ${r.created}, cập nhật ${r.updated}, bỏ qua ${r.skippedDone} buổi đã học`,
              )
              ;(r.failed ?? []).forEach((f: string) => message.warning(f, 6))
              load()
            } catch (e: any) {
              message.error(String(e.message ?? e), 8)
            } finally {
              setBusy(false)
            }
          }}
        >
          Đồng bộ lên lịch
        </Button>
        <Button
          loading={busy}
          onClick={async () => {
            const r = await post<{ removed: number }>(`/plans/${plan.id}/unsync`, {})
            message.success(`Đã gỡ ${r.removed} sự kiện`)
            load()
          }}
        >
          Gỡ khỏi lịch
        </Button>
      </Space>

      <List
        header={<b>{plan.title}</b>}
        bordered
        dataSource={sessions}
        renderItem={(s) => (
          <List.Item
            actions={[<a key="o" onClick={() => onOpenSession(s.id)}>Mở buổi học</a>]}
          >
            <List.Item.Meta
              title={
                <Space wrap>
                  <span>Buổi {s.ord + 1} · {s.date} {s.startHm}</span>
                  <Tag>{s.minutes} phút</Tag>
                  {s.status === 'done' && <Tag color="green">đã học</Tag>}
                  {s.eventId && <Tag color="purple">trên lịch</Tag>}
                </Space>
              }
              description={s.title}
            />
          </List.Item>
        )}
      />
    </Space>
  )
}
